//! The job engine: walks the source tree, performs the requested Windows
//! filesystem operation through the [`FileSystem`] seam, and reports
//! progress/conflicts/errors/completion through a [`JobSink`].
//!
//! This module performs real I/O (through the trait) and is explicitly
//! **not** part of `core::update`'s pure state machine — it is the
//! synchronous body `filecommand-tui` runs on a worker thread, mirroring how
//! `listing::list_dir_chunked` is the synchronous body driven from
//! `filecommand-tui`'s listing worker thread.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use crate::listing::DateTime;

use super::conflict::{ConflictChoice, ConflictInfo, ConflictPolicy, ConflictResolution};
use super::error::{ErrorChoice, ErrorInfo, ErrorPolicy, ErrorResolution, SkippedItem};
use super::fs::{is_cancelled, ChunkControl, FileId, FileSystem};
use super::job::{Job, JobKind, JobOutcome, JobSink, ProgressInfo};
use super::path::lexically_normalize;

const CHUNK_BYTES: usize = 256 * 1024;

pub fn run_job(job: &Job, fs: &dyn FileSystem, sink: &mut dyn JobSink) {
    match job.kind {
        JobKind::Mkdir => run_mkdir(job, fs, sink),
        JobKind::Delete => run_delete(job, fs, sink),
        JobKind::Copy => run_copy_or_move(job, fs, sink, false),
        JobKind::Move => run_copy_or_move(job, fs, sink, true),
        JobKind::Rename => run_rename(job, fs, sink),
    }
}

// ---------------------------------------------------------------------
// Shared per-file error-recovery loop
// ---------------------------------------------------------------------

enum RetryOutcome {
    Done,
    Skip(String),
    Abort,
}

fn attempt_with_recovery<F: FnMut() -> io::Result<()>>(
    path: &Path,
    error_policy: &mut ErrorPolicy,
    sink: &mut dyn JobSink,
    mut op: F,
) -> RetryOutcome {
    loop {
        match op() {
            Ok(()) => return RetryOutcome::Done,
            Err(e) => {
                let msg = e.to_string();
                let choice = match error_policy.resolve(ErrorInfo { path: path.to_path_buf(), message: msg.clone() }) {
                    ErrorResolution::Auto(c) => c,
                    ErrorResolution::Ask(info) => sink.error(info),
                };
                error_policy.apply(&choice);
                match choice {
                    ErrorChoice::Retry => continue,
                    ErrorChoice::Skip | ErrorChoice::SkipAll => return RetryOutcome::Skip(msg),
                    ErrorChoice::Abort => return RetryOutcome::Abort,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------
// Mkdir
// ---------------------------------------------------------------------

fn run_mkdir(job: &Job, fs: &dyn FileSystem, sink: &mut dyn JobSink) {
    let name = job.new_dir_name.clone().unwrap_or_default();
    let target = job.dest_dir.join(&name);
    sink.progress(ProgressInfo::starting(1, 0));
    let mut error_policy = ErrorPolicy::new();
    let outcome = match attempt_with_recovery(&target, &mut error_policy, sink, || fs.create_dir(&target)) {
        RetryOutcome::Done => JobOutcome::Completed { skipped: vec![] },
        RetryOutcome::Skip(reason) => JobOutcome::Cancelled { skipped: vec![SkippedItem::skipped(target.clone(), reason)] },
        RetryOutcome::Abort => JobOutcome::Cancelled { skipped: vec![] },
    };
    sink.done(outcome);
}

// ---------------------------------------------------------------------
// Delete
// ---------------------------------------------------------------------

enum DeleteItem {
    File { path: PathBuf, size: u64, readonly: bool },
    /// A reparse point (symlink/junction): removing it deletes the link,
    /// never the target's contents.
    ReparseLeaf { path: PathBuf, is_dir: bool },
    Dir { path: PathBuf },
}

fn walk_delete(fs: &dyn FileSystem, abs: &Path, out: &mut Vec<DeleteItem>, skipped: &mut Vec<SkippedItem>) {
    let md = match fs.metadata(abs) {
        Ok(m) => m,
        Err(e) => {
            skipped.push(SkippedItem::skipped(abs.to_path_buf(), e.to_string()));
            return;
        }
    };
    if md.reparse.is_reparse_point() {
        out.push(DeleteItem::ReparseLeaf { path: abs.to_path_buf(), is_dir: md.is_dir });
        return;
    }
    if md.is_dir {
        match fs.read_dir(abs) {
            Ok(entries) => {
                for e in entries {
                    walk_delete(fs, &abs.join(&e.name), out, skipped);
                }
                out.push(DeleteItem::Dir { path: abs.to_path_buf() });
            }
            Err(e) => skipped.push(SkippedItem::skipped(abs.to_path_buf(), e.to_string())),
        }
    } else {
        out.push(DeleteItem::File { path: abs.to_path_buf(), size: md.size, readonly: md.readonly });
    }
}

/// Remove a reparse point (symlink/junction) itself, never anything reached
/// through it: `remove_dir` for a directory-object link, `remove_file`
/// otherwise. Read-only is deliberately *not* cleared first — `set_readonly`
/// follows the link and would modify the target. Shared by Delete and the
/// source-removal half of a cross-volume move so both halves apply one rule
/// (move-reparse-point-safety design D3).
fn remove_reparse_leaf(fs: &dyn FileSystem, path: &Path, is_dir: bool) -> io::Result<()> {
    if is_dir {
        fs.remove_dir(path)
    } else {
        fs.remove_file(path)
    }
}

fn run_delete(job: &Job, fs: &dyn FileSystem, sink: &mut dyn JobSink) {
    let mut items = Vec::new();
    let mut skipped = Vec::new();
    for src in &job.sources {
        walk_delete(fs, &src.path, &mut items, &mut skipped);
    }

    let files_total = items.iter().filter(|i| !matches!(i, DeleteItem::Dir { .. })).count();
    let bytes_total: u64 = items.iter().map(|i| if let DeleteItem::File { size, .. } = i { *size } else { 0 }).sum();
    let mut progress = ProgressInfo::starting(files_total, bytes_total);
    sink.progress(progress.clone());

    let mut error_policy = ErrorPolicy::new();
    let mut cancelled = false;

    'items: for item in items {
        if sink.is_cancelled() {
            cancelled = true;
            break;
        }
        let (path, readonly, is_dir, size) = match &item {
            DeleteItem::File { path, size, readonly } => (path.clone(), *readonly, false, *size),
            DeleteItem::ReparseLeaf { path, is_dir } => (path.clone(), false, *is_dir, 0),
            DeleteItem::Dir { path } => (path.clone(), false, true, 0),
        };
        progress.current_file = path.file_name().unwrap_or_default().to_os_string();

        let outcome = attempt_with_recovery(&path, &mut error_policy, sink, || match &item {
            DeleteItem::ReparseLeaf { .. } => remove_reparse_leaf(fs, &path, is_dir),
            DeleteItem::File { .. } | DeleteItem::Dir { .. } => {
                if readonly {
                    let _ = fs.set_readonly(&path, false);
                }
                if is_dir {
                    fs.remove_dir(&path)
                } else {
                    fs.remove_file(&path)
                }
            }
        });
        match outcome {
            RetryOutcome::Done => {
                if !is_dir {
                    progress.files_done += 1;
                    progress.bytes_done += size;
                }
                sink.progress(progress.clone());
            }
            RetryOutcome::Skip(reason) => skipped.push(SkippedItem::skipped(path, reason)),
            RetryOutcome::Abort => {
                cancelled = true;
                break 'items;
            }
        }
    }

    sink.done(if cancelled { JobOutcome::Cancelled { skipped } } else { JobOutcome::Completed { skipped } });
}

// ---------------------------------------------------------------------
// Copy / Move
// ---------------------------------------------------------------------

/// How the source-removal half of a cross-volume move disposes of a planned
/// file once its copy succeeded (move-reparse-point-safety design D1). The
/// copy half is the same for every variant: the destination receives the
/// content read through the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceRemoval {
    /// A regular file not beneath a traversed link: clear read-only, then
    /// `remove_file`.
    RemoveFile,
    /// A file-type reparse point (file symlink) not beneath a link: its
    /// content was copied by following it; only the link is removed, with
    /// [`remove_reparse_leaf`].
    RemoveLink { is_dir: bool },
    /// Anything reached *through* a traversed directory link — the target's
    /// own files, nested links, nested directories. Never touched: it was
    /// never selected, only reached.
    Keep,
}

struct PlannedFile {
    src_abs: PathBuf,
    rel_path: PathBuf,
    size: u64,
    removal: SourceRemoval,
    /// Index into `CopyPlan::dirs` of the nearest enclosing
    /// `DirRemoval::RemoveLink` entry, if the file was planned through a
    /// link. Any outcome other than `Done` marks that link incomplete so it
    /// is kept (design D2).
    enclosing_link: Option<usize>,
}

/// How the move's post-loop cleanup disposes of a planned directory
/// (move-reparse-point-safety design D1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirRemoval {
    /// A real directory not beneath a link: best-effort `remove_dir`, so a
    /// directory left non-empty by a skipped file simply survives.
    RemoveIfEmpty,
    /// A directory-type reparse point: removed with [`remove_reparse_leaf`],
    /// and only when every file copied through it succeeded (design D2).
    RemoveLink { is_dir: bool },
    /// A directory beneath a traversed link. Still created at the
    /// destination (its `rel` is unchanged), never removed at the source.
    Keep,
}

struct PlannedDir {
    rel: PathBuf,
    /// Recorded during the walk rather than rebuilt as
    /// `source_dir.join(rel)`: the link's own path is needed for its
    /// removal, and this drops the assumption that a source's
    /// `original_name` sits directly in `source_dir`.
    src_abs: PathBuf,
    removal: DirRemoval,
}

struct CopyPlan {
    dirs: Vec<PlannedDir>,
    files: Vec<PlannedFile>,
    skipped: Vec<SkippedItem>,
}

fn plan_copy(fs: &dyn FileSystem, sources: &[super::job::SourceItem]) -> CopyPlan {
    let mut plan = CopyPlan { dirs: vec![], files: vec![], skipped: vec![] };
    let mut visited: HashSet<FileId> = HashSet::new();
    for src in sources {
        let rel_root = PathBuf::from(&src.original_name);
        walk_copy(fs, &src.path, &rel_root, None, &mut visited, &mut plan);
    }
    plan
}

/// `enclosing_link` is `Some(index into plan.dirs)` while the walk is beneath
/// a traversed directory-type reparse point — the outermost one, since that
/// is the only one the move may remove. Everything planned beneath it is
/// `Keep`. Copy totals and the cycle guard are unchanged by this bookkeeping
/// (move-reparse-point-safety design D1).
fn walk_copy(fs: &dyn FileSystem, abs: &Path, rel: &Path, enclosing_link: Option<usize>, visited: &mut HashSet<FileId>, plan: &mut CopyPlan) {
    let md = match fs.metadata(abs) {
        Ok(m) => m,
        Err(e) => {
            plan.skipped.push(SkippedItem::skipped(abs.to_path_buf(), e.to_string()));
            return;
        }
    };
    let under_link = enclosing_link.is_some();
    let is_link = md.reparse.is_reparse_point();

    // Reparse points duplicate their target's content by default: resolve
    // whether the target is a directory or a file via the *following*
    // metadata query.
    let is_dir_like = if is_link {
        fs.metadata_follow(abs).map(|m| m.is_dir).unwrap_or(false)
    } else {
        md.is_dir
    };

    if !is_dir_like {
        let size = if is_link {
            fs.metadata_follow(abs).map(|m| m.size).unwrap_or(md.size)
        } else {
            md.size
        };
        let removal = match (under_link, is_link) {
            (true, _) => SourceRemoval::Keep,
            (false, true) => SourceRemoval::RemoveLink { is_dir: md.is_dir },
            (false, false) => SourceRemoval::RemoveFile,
        };
        plan.files.push(PlannedFile { src_abs: abs.to_path_buf(), rel_path: rel.to_path_buf(), size, removal, enclosing_link });
        return;
    }

    // Directory (real or a directory-type reparse point/junction): guard
    // against recursion cycles — including junctions that point back inside
    // the tree already being copied — via the *real* (followed) identity.
    if let Some(id) = fs.metadata_follow(abs).ok().and_then(|m| m.file_id) {
        if !visited.insert(id) {
            plan.skipped.push(SkippedItem::skipped(abs.to_path_buf(), "skipped: reparse point forms a cycle back into the source tree".to_string()));
            return;
        }
    }

    // `is_dir` here is the link's *own* directory-ness (a junction or
    // `mklink /D` symlink on Windows; never for a Unix symlink), which is
    // exactly what decides `remove_dir` versus `remove_file` for the leaf
    // (design D4).
    let removal = match (under_link, is_link) {
        (true, _) => DirRemoval::Keep,
        (false, true) => DirRemoval::RemoveLink { is_dir: md.is_dir },
        (false, false) => DirRemoval::RemoveIfEmpty,
    };
    let index = plan.dirs.len();
    plan.dirs.push(PlannedDir { rel: rel.to_path_buf(), src_abs: abs.to_path_buf(), removal });
    let child_link = match removal {
        DirRemoval::RemoveLink { .. } => Some(index),
        _ => enclosing_link,
    };
    match fs.read_dir(abs) {
        Ok(entries) => {
            for entry in entries {
                walk_copy(fs, &abs.join(&entry.name), &rel.join(&entry.name), child_link, visited, plan);
            }
        }
        Err(e) => plan.skipped.push(SkippedItem::skipped(abs.to_path_buf(), e.to_string())),
    }
}

enum CopyOneResult {
    Done(u64),
    Cancelled,
    Failed(String),
    Aborted,
}

#[allow(clippy::too_many_arguments)]
fn copy_one_file(
    fs: &dyn FileSystem,
    src: &Path,
    dst: &Path,
    error_policy: &mut ErrorPolicy,
    sink: &mut dyn JobSink,
    progress: &mut ProgressInfo,
    bytes_done_baseline: u64,
    streams_supported: bool,
    streams_not_copied: &mut usize,
    warnings: &mut Vec<SkippedItem>,
) -> CopyOneResult {
    loop {
        let result = fs.copy_file_chunked(src, dst, CHUNK_BYTES, &mut |done| {
            progress.bytes_done = bytes_done_baseline + done;
            sink.progress(progress.clone());
            if sink.is_cancelled() {
                ChunkControl::Cancel
            } else {
                ChunkControl::Continue
            }
        });
        match result {
            Ok(bytes) => {
                restore_metadata(fs, src, dst, streams_supported, streams_not_copied, warnings);
                return CopyOneResult::Done(bytes);
            }
            Err(e) if is_cancelled(&e) => return CopyOneResult::Cancelled,
            Err(e) => {
                let msg = e.to_string();
                let choice = match error_policy.resolve(ErrorInfo { path: src.to_path_buf(), message: msg.clone() }) {
                    ErrorResolution::Auto(c) => c,
                    ErrorResolution::Ask(info) => sink.error(info),
                };
                error_policy.apply(&choice);
                match choice {
                    ErrorChoice::Retry => continue,
                    ErrorChoice::Skip | ErrorChoice::SkipAll => return CopyOneResult::Failed(msg),
                    ErrorChoice::Abort => return CopyOneResult::Aborted,
                }
            }
        }
    }
}

/// After a successful content copy, carry over what the plain copy does
/// not: named alternate streams, the modification time, and the read-only
/// attribute. Every failure is recorded as a [`SkippedItem`] *warning*
/// against the destination path rather than silently discarded — the file
/// itself still counts as copied (error-diagnostics design D5;
/// operation-dialogs "Metadata-restore failures are reported in the
/// end-of-job summary").
///
/// When the destination volume has no named-stream support at all, per-file
/// stream copies are skipped entirely and the file is counted in
/// `streams_not_copied` instead, so the job reports one aggregated warning
/// rather than one per file.
fn restore_metadata(
    fs: &dyn FileSystem,
    src: &Path,
    dst: &Path,
    streams_supported: bool,
    streams_not_copied: &mut usize,
    warnings: &mut Vec<SkippedItem>,
) {
    match fs.list_streams(src) {
        Ok(streams) => {
            if streams_supported {
                for name in streams {
                    if let Err(e) = fs.copy_stream_chunked(src, &name, dst, CHUNK_BYTES) {
                        warnings.push(SkippedItem::warning(dst.to_path_buf(), format!("stream {} not copied: {e}", name.to_string_lossy())));
                    }
                }
            } else if !streams.is_empty() {
                *streams_not_copied += 1;
            }
        }
        Err(e) => warnings.push(SkippedItem::warning(dst.to_path_buf(), format!("alternate data streams not listed: {e}"))),
    }
    if let Ok(md) = fs.metadata_follow(src) {
        if let Some(modified) = md.modified {
            if let Err(e) = fs.set_modified(dst, modified) {
                warnings.push(SkippedItem::warning(dst.to_path_buf(), format!("modification time not restored: {e}")));
            }
        }
        if md.readonly {
            if let Err(e) = fs.set_readonly(dst, true) {
                warnings.push(SkippedItem::warning(dst.to_path_buf(), format!("read-only attribute not restored: {e}")));
            }
        }
    }
}

fn identity_matches(fs: &dyn FileSystem, a: &Path, b_id: Option<FileId>) -> bool {
    matches!((fs.metadata_follow(a).ok().and_then(|m| m.file_id), b_id), (Some(x), Some(y)) if x == y)
}

/// Whether `target` resolves to the same file as `src` (file-operations "A
/// copy never overwrites its own source"). Unlike [`identity_matches`], the
/// target's identity is *followed*, so a file symlink pointing at the source
/// also counts. When either identity is unknown, the decision falls back to
/// comparing the lexically-normalized paths. A target that does not exist is
/// never the same file.
fn same_file(fs: &dyn FileSystem, src: &Path, target: &Path) -> bool {
    let src_id = fs.metadata_follow(src).ok().and_then(|m| m.file_id);
    let target_id = match fs.metadata_follow(target) {
        Ok(md) => md.file_id,
        Err(_) => return false,
    };
    match (src_id, target_id) {
        (Some(a), Some(b)) => a == b,
        _ => normalized_paths_equal(src, target),
    }
}

/// [`same_file`]'s fallback when a file identity cannot be read: the two
/// paths are equal after lexical normalization, compared case-insensitively
/// on Windows (where a case-insensitive volume is the norm) and exactly
/// elsewhere. Meaningful only for absolute paths; a relative path simply
/// does not match, which is no worse than no fallback at all.
fn normalized_paths_equal(a: &Path, b: &Path) -> bool {
    let a = lexically_normalize(a);
    let b = lexically_normalize(b);
    #[cfg(windows)]
    {
        let a = a.as_os_str().to_string_lossy().to_lowercase();
        let b = b.as_os_str().to_string_lossy().to_lowercase();
        a == b
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

fn run_copy_or_move(job: &Job, fs: &dyn FileSystem, sink: &mut dyn JobSink, is_move: bool) {
    if is_move && !job.sources.is_empty() && job.sources.iter().all(|s| fs.same_volume(&s.path, &job.dest_dir).unwrap_or(false)) {
        run_move_same_volume(job, fs, sink);
        return;
    }

    let plan = plan_copy(fs, &job.sources);
    let files_total = plan.files.len();
    let bytes_total: u64 = plan.files.iter().map(|f| f.size).sum();
    let mut progress = ProgressInfo::starting(files_total, bytes_total);
    sink.progress(progress.clone());

    let mut conflict_policy = ConflictPolicy::new();
    let mut error_policy = ErrorPolicy::new();
    let mut skipped = plan.skipped;
    let mut cancelled = false;
    // Asked once per job (error-diagnostics design D5): a destination
    // volume without named-stream support (FAT32/exFAT) would otherwise
    // produce one stream warning per file. An error answers "supported", so
    // genuine per-file failures still surface individually. The aggregate
    // count is reported as a single job-level warning below.
    let streams_supported = fs.supports_named_streams(&job.dest_dir).unwrap_or(true);
    let mut streams_not_copied = 0usize;
    // Per planned directory: whether any file copied through that link
    // ended in anything other than `Done`. A marked link is kept, the same
    // way a real directory survives a skipped file (move-reparse-point-
    // safety design D2). Only `DirRemoval::RemoveLink` entries are ever
    // read.
    let mut link_incomplete = vec![false; plan.dirs.len()];

    'dirs: for planned_dir in &plan.dirs {
        if sink.is_cancelled() {
            cancelled = true;
            break;
        }
        let target = job.dest_dir.join(&planned_dir.rel);
        let outcome = attempt_with_recovery(&target, &mut error_policy, sink, || match fs.create_dir(&target) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(e),
        });
        match outcome {
            RetryOutcome::Done => {}
            RetryOutcome::Skip(reason) => skipped.push(SkippedItem::skipped(target, reason)),
            RetryOutcome::Abort => {
                cancelled = true;
                break 'dirs;
            }
        }
    }

    if !cancelled {
        'files: for planned in &plan.files {
            if sink.is_cancelled() {
                cancelled = true;
                break;
            }
            let target = job.dest_dir.join(&planned.rel_path);
            progress.current_file = planned.rel_path.file_name().unwrap_or_default().to_os_string();

            // A copy never writes to its own source: creating the target
            // would truncate the source before the first byte is read, and
            // the copy half of a move would then delete it. Unlike
            // `run_move_same_volume`/`run_rename`, where identity-equality
            // means "no conflict" so case-only renames pass, here the same
            // file is skipped outright — nothing is written, no conflict is
            // raised, and a move's source removal is never reached.
            if same_file(fs, &planned.src_abs, &target) {
                skipped.push(SkippedItem::skipped(planned.src_abs.clone(), "source and destination are the same file".to_string()));
                if let Some(link) = planned.enclosing_link {
                    link_incomplete[link] = true;
                }
                continue 'files;
            }

            let conflict = match fs.metadata(&target) {
                Ok(target_md) if !identity_matches(fs, &planned.src_abs, target_md.file_id) => Some(ConflictInfo {
                    source_name: planned.rel_path.file_name().unwrap_or_default().to_os_string(),
                    source_size: planned.size,
                    source_modified: fs.metadata_follow(&planned.src_abs).ok().and_then(|m| m.modified).map(DateTime::from_system_time),
                    target_path: target.clone(),
                    target_size: target_md.size,
                    target_modified: target_md.modified.map(DateTime::from_system_time),
                }),
                _ => None,
            };

            let mut final_target = target.clone();
            let mut do_copy = true;
            if let Some(info) = conflict {
                let choice = match conflict_policy.resolve(info.clone()) {
                    ConflictResolution::Auto(c) => c,
                    ConflictResolution::Ask(i) => sink.conflict(i),
                };
                conflict_policy.apply(&choice);
                match choice {
                    ConflictChoice::Overwrite | ConflictChoice::OverwriteAll => {
                        let _ = fs.set_readonly(&target, false);
                    }
                    ConflictChoice::Skip | ConflictChoice::SkipAll => {
                        skipped.push(SkippedItem::skipped(target, "target already exists".to_string()));
                        do_copy = false;
                        if let Some(link) = planned.enclosing_link {
                            link_incomplete[link] = true;
                        }
                    }
                    ConflictChoice::Rename(new_name) => {
                        final_target = target.parent().unwrap_or(&job.dest_dir).join(new_name);
                    }
                    // Abort from the prompt: the conflicting target is left
                    // as it is and not recorded as skipped, nothing after it
                    // is processed (operation-dialogs "Overwrite-conflict
                    // dialog"; process-lifecycle-hardening design D7).
                    ConflictChoice::Cancel => {
                        cancelled = true;
                        break 'files;
                    }
                }
            }

            if do_copy {
                let baseline = progress.bytes_done;
                match copy_one_file(fs, &planned.src_abs, &final_target, &mut error_policy, sink, &mut progress, baseline, streams_supported, &mut streams_not_copied, &mut skipped) {
                    CopyOneResult::Done(bytes) => {
                        progress.files_done += 1;
                        progress.bytes_done = baseline + bytes;
                        sink.progress(progress.clone());
                        if is_move {
                            match planned.removal {
                                SourceRemoval::RemoveFile => {
                                    let _ = fs.set_readonly(&planned.src_abs, false);
                                    let _ = fs.remove_file(&planned.src_abs);
                                }
                                // A file symlink: the copy followed it; only
                                // the link goes, and a failure is reported
                                // rather than ignored (design D3).
                                SourceRemoval::RemoveLink { is_dir } => {
                                    if let Err(e) = remove_reparse_leaf(fs, &planned.src_abs, is_dir) {
                                        skipped.push(SkippedItem::skipped(planned.src_abs.clone(), format!("copied, but the link could not be removed: {e}")));
                                    }
                                }
                                // Reached through a link: never selected, never
                                // removed (R-01).
                                SourceRemoval::Keep => {}
                            }
                        }
                    }
                    CopyOneResult::Cancelled | CopyOneResult::Aborted => {
                        cancelled = true;
                        break 'files;
                    }
                    CopyOneResult::Failed(reason) => {
                        progress.bytes_done = baseline;
                        skipped.push(SkippedItem::skipped(planned.src_abs.clone(), reason));
                        if let Some(link) = planned.enclosing_link {
                            link_incomplete[link] = true;
                        }
                    }
                }
            }
        }
    }

    if is_move && !cancelled {
        // Clean up the source, deepest first, on each entry's recorded path
        // (move-reparse-point-safety design D1-D3).
        for (index, planned_dir) in plan.dirs.iter().enumerate().rev() {
            match planned_dir.removal {
                // Best-effort: a directory left non-empty by a skipped file
                // simply survives, and that is an expected outcome.
                DirRemoval::RemoveIfEmpty => {
                    let _ = fs.remove_dir(&planned_dir.src_abs);
                }
                // Beneath a link: the target's own directories stay.
                DirRemoval::Keep => {}
                // The link itself goes only once everything copied through
                // it succeeded, so the user is never left with the link
                // gone and a partial copy at the destination. A failed
                // removal is not an expected outcome, so it is reported.
                DirRemoval::RemoveLink { is_dir } => {
                    if link_incomplete[index] {
                        continue;
                    }
                    if let Err(e) = remove_reparse_leaf(fs, &planned_dir.src_abs, is_dir) {
                        skipped.push(SkippedItem::skipped(planned_dir.src_abs.clone(), format!("copied, but the link could not be removed: {e}")));
                    }
                }
            }
        }
    }

    // One aggregated warning when the destination volume cannot hold named
    // streams at all, rather than one warning per file (error-diagnostics
    // design D5; operation-dialogs "Metadata-restore failures are reported
    // in the end-of-job summary": "A destination without stream support
    // yields one warning").
    if streams_not_copied > 0 {
        skipped.push(SkippedItem::warning(
            job.dest_dir.clone(),
            format!("alternate data streams not supported; streams of {streams_not_copied} file(s) not copied"),
        ));
    }

    sink.done(if cancelled { JobOutcome::Cancelled { skipped } } else { JobOutcome::Completed { skipped } });
}

fn run_move_same_volume(job: &Job, fs: &dyn FileSystem, sink: &mut dyn JobSink) {
    let files_total = job.sources.len();
    let mut progress = ProgressInfo::starting(files_total, 0);
    sink.progress(progress.clone());

    let mut error_policy = ErrorPolicy::new();
    let mut conflict_policy = ConflictPolicy::new();
    let mut skipped = Vec::new();
    let mut cancelled = false;

    'items: for src in &job.sources {
        if sink.is_cancelled() {
            cancelled = true;
            break;
        }
        progress.current_file = src.original_name.clone();
        let target = job.dest_dir.join(&src.original_name);
        let mut final_target = target.clone();

        if let Ok(target_md) = fs.metadata(&target) {
            let src_id = fs.metadata(&src.path).ok().and_then(|m| m.file_id);
            let same_identity = matches!((src_id, target_md.file_id), (Some(a), Some(b)) if a == b);
            if !same_identity {
                let info = ConflictInfo {
                    source_name: src.original_name.clone(),
                    source_size: fs.metadata(&src.path).map(|m| m.size).unwrap_or(0),
                    source_modified: fs.metadata(&src.path).ok().and_then(|m| m.modified).map(DateTime::from_system_time),
                    target_path: target.clone(),
                    target_size: target_md.size,
                    target_modified: target_md.modified.map(DateTime::from_system_time),
                };
                let choice = match conflict_policy.resolve(info.clone()) {
                    ConflictResolution::Auto(c) => c,
                    ConflictResolution::Ask(i) => sink.conflict(i),
                };
                conflict_policy.apply(&choice);
                match choice {
                    ConflictChoice::Overwrite | ConflictChoice::OverwriteAll => {
                        let _ = fs.set_readonly(&target, false);
                        let _ = if target_md.is_dir { fs.remove_dir(&target) } else { fs.remove_file(&target) };
                    }
                    ConflictChoice::Skip | ConflictChoice::SkipAll => {
                        skipped.push(SkippedItem::skipped(target, "target already exists".to_string()));
                        continue 'items;
                    }
                    ConflictChoice::Rename(new_name) => final_target = job.dest_dir.join(new_name),
                    ConflictChoice::Cancel => {
                        cancelled = true;
                        break 'items;
                    }
                }
            }
        }

        let outcome = attempt_with_recovery(&src.path, &mut error_policy, sink, || fs.rename(&src.path, &final_target));
        match outcome {
            RetryOutcome::Done => {
                progress.files_done += 1;
                sink.progress(progress.clone());
            }
            RetryOutcome::Skip(reason) => skipped.push(SkippedItem::skipped(src.path.clone(), reason)),
            RetryOutcome::Abort => {
                cancelled = true;
                break 'items;
            }
        }
    }

    sink.done(if cancelled { JobOutcome::Cancelled { skipped } } else { JobOutcome::Completed { skipped } });
}

// ---------------------------------------------------------------------
// Rename (file-action-menu "In-place Rename")
// ---------------------------------------------------------------------

/// In-place rename of the job's single source to `dest_dir` joined with
/// `new_dir_name` — `dest_dir` is the resolved parent of the new name, equal
/// to `source_dir` for a plain name, per the `Job` doc. Mirrors
/// [`run_move_same_volume`]'s identity-aware conflict check — a case-only
/// rename onto the same underlying file is never treated as "target already
/// exists" — but always targets `new_dir_name` rather than the source's own
/// original name, and never falls back to a copy-then-delete cross-volume
/// path since a rename never leaves its own directory. Collisions and errors
/// surface through the same `sink.conflict`/`sink.error` calls every other
/// job uses, so the existing `operation-dialogs` conflict/error dialogs
/// render unchanged (file-action-menu "Rename collisions/failures must
/// surface through the existing overwrite-conflict and error-recovery
/// dialogs").
fn run_rename(job: &Job, fs: &dyn FileSystem, sink: &mut dyn JobSink) {
    let mut progress = ProgressInfo::starting(1, 0);
    sink.progress(progress.clone());

    let Some(src) = job.sources.first() else {
        sink.done(JobOutcome::Completed { skipped: vec![] });
        return;
    };
    let new_name = job.new_dir_name.clone().unwrap_or_default();
    progress.current_file = new_name.clone();

    let target = job.dest_dir.join(&new_name);
    let mut final_target = target.clone();
    let mut skipped = Vec::new();

    if let Ok(target_md) = fs.metadata(&target) {
        let src_id = fs.metadata(&src.path).ok().and_then(|m| m.file_id);
        let same_identity = matches!((src_id, target_md.file_id), (Some(a), Some(b)) if a == b);
        if !same_identity {
            let info = ConflictInfo {
                source_name: src.original_name.clone(),
                source_size: fs.metadata(&src.path).map(|m| m.size).unwrap_or(0),
                source_modified: fs.metadata(&src.path).ok().and_then(|m| m.modified).map(DateTime::from_system_time),
                target_path: target.clone(),
                target_size: target_md.size,
                target_modified: target_md.modified.map(DateTime::from_system_time),
            };
            match sink.conflict(info) {
                ConflictChoice::Overwrite | ConflictChoice::OverwriteAll => {
                    let _ = fs.set_readonly(&target, false);
                    let _ = if target_md.is_dir { fs.remove_dir(&target) } else { fs.remove_file(&target) };
                }
                ConflictChoice::Skip | ConflictChoice::SkipAll => {
                    skipped.push(SkippedItem::skipped(target, "target already exists".to_string()));
                    sink.done(JobOutcome::Completed { skipped });
                    return;
                }
                ConflictChoice::Rename(renamed_to) => final_target = job.dest_dir.join(renamed_to),
                ConflictChoice::Cancel => {
                    sink.done(JobOutcome::Cancelled { skipped });
                    return;
                }
            }
        }
    }

    let mut error_policy = ErrorPolicy::new();
    let outcome = attempt_with_recovery(&src.path, &mut error_policy, sink, || fs.rename(&src.path, &final_target));
    let job_outcome = match outcome {
        RetryOutcome::Done => {
            progress.files_done = 1;
            sink.progress(progress.clone());
            JobOutcome::Completed { skipped }
        }
        RetryOutcome::Skip(reason) => {
            skipped.push(SkippedItem::skipped(src.path.clone(), reason));
            JobOutcome::Completed { skipped }
        }
        RetryOutcome::Abort => JobOutcome::Cancelled { skipped },
    };
    sink.done(job_outcome);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs_ops::error::ItemKind;
    use crate::fs_ops::fs::{FakeFs, FsMetadata, InjectableOp, RawEntry};
    use crate::fs_ops::job::{CancelFlag, SourceItem};
    use std::ffi::{OsStr, OsString};
    use std::sync::{Arc, Mutex};
    use std::time::SystemTime;

    /// A [`JobSink`] test double that records every event and auto-resolves
    /// conflicts/errors from a scripted queue (defaulting to Overwrite/Skip
    /// respectively if the queue runs dry).
    struct RecordingSink {
        pub events: Vec<String>,
        pub conflict_answers: Vec<ConflictChoice>,
        pub error_answers: Vec<ErrorChoice>,
        pub cancel: CancelFlag,
        pub outcome: Option<JobOutcome>,
        pub conflicts_seen: usize,
    }

    impl RecordingSink {
        fn new() -> Self {
            RecordingSink { events: vec![], conflict_answers: vec![], error_answers: vec![], cancel: CancelFlag::new(), outcome: None, conflicts_seen: 0 }
        }
    }

    impl JobSink for RecordingSink {
        fn progress(&mut self, info: ProgressInfo) {
            self.events.push(format!("progress:{}/{}", info.files_done, info.files_total));
        }
        fn conflict(&mut self, _info: ConflictInfo) -> ConflictChoice {
            self.conflicts_seen += 1;
            if self.conflict_answers.is_empty() {
                ConflictChoice::Overwrite
            } else {
                self.conflict_answers.remove(0)
            }
        }
        fn error(&mut self, _info: ErrorInfo) -> ErrorChoice {
            if self.error_answers.is_empty() {
                ErrorChoice::Skip
            } else {
                self.error_answers.remove(0)
            }
        }
        fn done(&mut self, outcome: JobOutcome) {
            self.outcome = Some(outcome);
        }
        fn is_cancelled(&self) -> bool {
            self.cancel.is_cancelled()
        }
    }

    fn source(name: &str, path: &str) -> SourceItem {
        SourceItem { original_name: OsString::from(name), path: PathBuf::from(path), is_dir: false }
    }

    #[test]
    fn mkdir_creates_directory() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/panel"));
        let job = Job {
            kind: JobKind::Mkdir,
            sources: vec![],
            source_dir: PathBuf::from("/panel"),
            dest_dir: PathBuf::from("/panel"),
            new_dir_name: Some(OsString::from("newdir")),
        };
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);
        assert!(matches!(sink.outcome, Some(JobOutcome::Completed { .. })));
        assert!(fake.metadata(Path::new("/panel/newdir")).unwrap().is_dir);
    }

    #[test]
    fn copy_multi_file_tree() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/src"));
        fake.add_dir(Path::new("/src/sub"));
        fake.add_file(Path::new("/src/a.txt"), 10);
        fake.add_file(Path::new("/src/sub/b.txt"), 20);
        fake.add_dir(Path::new("/dst"));

        let job = Job {
            kind: JobKind::Copy,
            sources: vec![source("src", "/src")],
            source_dir: PathBuf::from("/"),
            dest_dir: PathBuf::from("/dst"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);
        assert!(matches!(sink.outcome, Some(JobOutcome::Completed { skipped }) if skipped.is_empty()));
        assert_eq!(fake.metadata(Path::new("/dst/src/a.txt")).unwrap().size, 10);
        assert_eq!(fake.metadata(Path::new("/dst/src/sub/b.txt")).unwrap().size, 20);
        // Source is untouched by a copy.
        assert!(fake.metadata(Path::new("/src/a.txt")).is_ok());
    }

    #[test]
    fn move_same_volume_is_instant_rename() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/src"));
        fake.add_file(Path::new("/src/a.txt"), 5);
        fake.add_dir(Path::new("/dst"));

        let job = Job {
            kind: JobKind::Move,
            sources: vec![source("a.txt", "/src/a.txt")],
            source_dir: PathBuf::from("/src"),
            dest_dir: PathBuf::from("/dst"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);
        assert!(matches!(sink.outcome, Some(JobOutcome::Completed { .. })));
        assert!(fake.metadata(Path::new("/dst/a.txt")).is_ok());
        assert!(fake.metadata(Path::new("/src/a.txt")).is_err());
    }

    #[test]
    fn move_cross_volume_copies_then_deletes_source() {
        let fake = FakeFs::new();
        fake.set_volume(Path::new("/v1"), 1);
        fake.set_volume(Path::new("/v2"), 2);
        fake.add_dir(Path::new("/v1/src"));
        fake.add_file(Path::new("/v1/src/a.txt"), 7);
        fake.add_dir(Path::new("/v2/dst"));

        let job = Job {
            kind: JobKind::Move,
            sources: vec![source("src", "/v1/src")],
            source_dir: PathBuf::from("/v1"),
            dest_dir: PathBuf::from("/v2/dst"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);
        assert!(matches!(sink.outcome, Some(JobOutcome::Completed { .. })));
        assert!(fake.metadata(Path::new("/v2/dst/src/a.txt")).is_ok());
        assert!(fake.metadata(Path::new("/v1/src/a.txt")).is_err());
        assert!(fake.metadata(Path::new("/v1/src")).is_err(), "now-empty source dir should be cleaned up");
    }

    #[test]
    fn move_cross_volume_leaves_source_intact_on_failed_copy() {
        let fake = FakeFs::new();
        fake.set_volume(Path::new("/v1"), 1);
        fake.set_volume(Path::new("/v2"), 2);
        fake.add_file(Path::new("/v1/a.txt"), 100);
        fake.add_dir(Path::new("/v2"));
        fake.inject(super::super::fs::InjectableOp::CopyAfterChunks(0), Path::new("/v2/a.txt"), io::ErrorKind::StorageFull);

        let job = Job {
            kind: JobKind::Move,
            sources: vec![source("a.txt", "/v1/a.txt")],
            source_dir: PathBuf::from("/v1"),
            dest_dir: PathBuf::from("/v2"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        sink.error_answers.push(ErrorChoice::Skip);
        run_job(&job, &fake, &mut sink);
        assert!(fake.metadata(Path::new("/v1/a.txt")).is_ok(), "source must survive a failed copy");
        match sink.outcome {
            Some(JobOutcome::Completed { skipped }) => assert_eq!(skipped.len(), 1),
            other => panic!("expected Completed with one skip, got {other:?}"),
        }
    }

    #[test]
    fn delete_removes_tree_bottom_up() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/d"));
        fake.add_dir(Path::new("/d/sub"));
        fake.add_file(Path::new("/d/sub/f.txt"), 3);

        let job = Job {
            kind: JobKind::Delete,
            sources: vec![source("d", "/d")],
            source_dir: PathBuf::from("/"),
            dest_dir: PathBuf::from("/"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);
        assert!(matches!(sink.outcome, Some(JobOutcome::Completed { .. })));
        assert!(fake.metadata(Path::new("/d")).is_err());
    }

    #[test]
    fn delete_clears_readonly_before_removal() {
        let fake = FakeFs::new();
        fake.add_file(Path::new("/ro.txt"), 1);
        fake.set_readonly(Path::new("/ro.txt"), true).unwrap();
        let job = Job {
            kind: JobKind::Delete,
            sources: vec![source("ro.txt", "/ro.txt")],
            source_dir: PathBuf::from("/"),
            dest_dir: PathBuf::from("/"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);
        assert!(matches!(sink.outcome, Some(JobOutcome::Completed { .. })));
        assert!(fake.metadata(Path::new("/ro.txt")).is_err());
    }

    #[test]
    fn reparse_point_delete_removes_link_not_target() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/real"));
        fake.add_file(Path::new("/real/f.txt"), 1);
        fake.add_reparse_point(Path::new("/link"), true, Path::new("/real"), super::super::fs::ReparseKind::Junction);
        let job = Job {
            kind: JobKind::Delete,
            sources: vec![source("link", "/link")],
            source_dir: PathBuf::from("/"),
            dest_dir: PathBuf::from("/"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);
        assert!(fake.metadata(Path::new("/link")).is_err(), "the link itself is gone");
        assert!(fake.metadata(Path::new("/real/f.txt")).is_ok(), "target content must survive");
    }

    #[test]
    fn reparse_point_copy_duplicates_target_content() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/real"));
        fake.add_file(Path::new("/real/f.txt"), 9);
        fake.add_reparse_point(Path::new("/link"), true, Path::new("/real"), super::super::fs::ReparseKind::Junction);
        fake.add_dir(Path::new("/dst"));
        let job = Job {
            kind: JobKind::Copy,
            sources: vec![source("link", "/link")],
            source_dir: PathBuf::from("/"),
            dest_dir: PathBuf::from("/dst"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);
        assert!(matches!(sink.outcome, Some(JobOutcome::Completed { .. })));
        assert_eq!(fake.metadata(Path::new("/dst/link/f.txt")).unwrap().size, 9);
    }

    #[test]
    fn reparse_point_cycle_is_skipped_not_infinite() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/src"));
        fake.add_file(Path::new("/src/a.txt"), 1);
        // A junction inside /src that points right back at /src.
        fake.add_reparse_point(Path::new("/src/loop"), true, Path::new("/src"), super::super::fs::ReparseKind::Junction);
        fake.add_dir(Path::new("/dst"));
        let job = Job {
            kind: JobKind::Copy,
            sources: vec![source("src", "/src")],
            source_dir: PathBuf::from("/"),
            dest_dir: PathBuf::from("/dst"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);
        match sink.outcome {
            Some(JobOutcome::Completed { skipped }) => assert_eq!(skipped.len(), 1),
            other => panic!("expected a single skipped cycle entry, got {other:?}"),
        }
        assert!(fake.metadata(Path::new("/dst/src/a.txt")).is_ok());
    }

    // -------------------------------------------------------------
    // move-reparse-point-safety (file-operations "Reparse-point
    // (symlink/junction) semantics": the cross-volume move's
    // source-removal half treats every link as a leaf)
    // -------------------------------------------------------------

    /// Volumes `/v1` and `/v2`; `/v1/work/proj/file.txt`; `/v1/shared/lib.dll`
    /// and `/v1/shared/x86/a.dll`; junction `/v1/work/proj/shared` → `/v1/shared`;
    /// empty `/v2/backup`.
    fn junction_in_tree_fixture() -> FakeFs {
        let fake = FakeFs::new();
        fake.set_volume(Path::new("/v1"), 1);
        fake.set_volume(Path::new("/v2"), 2);
        fake.add_dir(Path::new("/v1/work"));
        fake.add_dir(Path::new("/v1/work/proj"));
        fake.add_file(Path::new("/v1/work/proj/file.txt"), 3);
        fake.add_dir(Path::new("/v1/shared"));
        fake.add_file(Path::new("/v1/shared/lib.dll"), 10);
        fake.add_dir(Path::new("/v1/shared/x86"));
        fake.add_file(Path::new("/v1/shared/x86/a.dll"), 20);
        fake.add_reparse_point(Path::new("/v1/work/proj/shared"), true, Path::new("/v1/shared"), super::super::fs::ReparseKind::Junction);
        fake.add_dir(Path::new("/v2/backup"));
        fake
    }

    fn move_proj_to_backup() -> Job {
        Job {
            kind: JobKind::Move,
            sources: vec![source("proj", "/v1/work/proj")],
            source_dir: PathBuf::from("/v1/work"),
            dest_dir: PathBuf::from("/v2/backup"),
            new_dir_name: None,
        }
    }

    fn assert_shared_target_intact(fake: &FakeFs) {
        assert_eq!(fake.metadata(Path::new("/v1/shared/lib.dll")).unwrap().size, 10, "R-01: the junction target's file must survive the move");
        assert_eq!(fake.metadata(Path::new("/v1/shared/x86/a.dll")).unwrap().size, 20, "R-01: the target's subdirectory file must survive");
        assert!(fake.metadata(Path::new("/v1/shared/x86")).unwrap().is_dir, "R-01: the target's subdirectory must survive");
        assert!(fake.metadata(Path::new("/v1/shared")).unwrap().is_dir);
    }

    /// Audit finding R-01: "Cross-volume move of a tree containing a
    /// junction to an outside directory leaves that directory's files
    /// intact".
    #[test]
    fn move_cross_volume_through_junction_leaves_target_files_intact() {
        let fake = junction_in_tree_fixture();
        let mut sink = RecordingSink::new();
        run_job(&move_proj_to_backup(), &fake, &mut sink);

        assert!(matches!(&sink.outcome, Some(JobOutcome::Completed { skipped }) if skipped.is_empty()), "{:?}", sink.outcome);
        assert_shared_target_intact(&fake);
        assert_eq!(fake.metadata(Path::new("/v2/backup/proj/file.txt")).unwrap().size, 3);
        assert_eq!(fake.metadata(Path::new("/v2/backup/proj/shared/lib.dll")).unwrap().size, 10, "the destination receives the link target's content");
        assert_eq!(fake.metadata(Path::new("/v2/backup/proj/shared/x86/a.dll")).unwrap().size, 20);
        assert!(fake.metadata(Path::new("/v1/work/proj/shared")).is_err(), "the junction itself is removed");
        assert!(fake.metadata(Path::new("/v1/work/proj")).is_err(), "the source tree is gone");
    }

    /// "Cross-volume move of a selected junction removes only the link".
    #[test]
    fn move_cross_volume_of_selected_junction_removes_only_the_link() {
        let fake = FakeFs::new();
        fake.set_volume(Path::new("/v1"), 1);
        fake.set_volume(Path::new("/v2"), 2);
        fake.add_dir(Path::new("/v1/data"));
        fake.add_file(Path::new("/v1/data/a.txt"), 5);
        fake.add_dir(Path::new("/v1/data/sub"));
        fake.add_file(Path::new("/v1/data/sub/b.txt"), 6);
        fake.add_dir(Path::new("/v1/work"));
        fake.add_reparse_point(Path::new("/v1/work/link"), true, Path::new("/v1/data"), super::super::fs::ReparseKind::Junction);
        fake.add_dir(Path::new("/v2/backup"));

        let job = Job {
            kind: JobKind::Move,
            sources: vec![source("link", "/v1/work/link")],
            source_dir: PathBuf::from("/v1/work"),
            dest_dir: PathBuf::from("/v2/backup"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);

        assert!(matches!(&sink.outcome, Some(JobOutcome::Completed { skipped }) if skipped.is_empty()), "{:?}", sink.outcome);
        assert_eq!(fake.metadata(Path::new("/v2/backup/link/a.txt")).unwrap().size, 5, "the destination receives the target's content");
        assert_eq!(fake.metadata(Path::new("/v2/backup/link/sub/b.txt")).unwrap().size, 6);
        assert!(fake.metadata(Path::new("/v1/work/link")).is_err(), "the junction is removed");
        assert_eq!(fake.metadata(Path::new("/v1/data/a.txt")).unwrap().size, 5, "the target's files are untouched");
        assert_eq!(fake.metadata(Path::new("/v1/data/sub/b.txt")).unwrap().size, 6);
        assert!(fake.metadata(Path::new("/v1/data/sub")).unwrap().is_dir);
    }

    /// "A link whose content was not fully copied is kept".
    #[test]
    fn move_cross_volume_keeps_the_junction_when_a_file_beneath_it_was_skipped() {
        let fake = junction_in_tree_fixture();
        fake.inject(InjectableOp::CopyAfterChunks(0), Path::new("/v2/backup/proj/shared/lib.dll"), io::ErrorKind::StorageFull);
        let mut sink = RecordingSink::new();
        sink.error_answers.push(ErrorChoice::Skip);
        run_job(&move_proj_to_backup(), &fake, &mut sink);

        match &sink.outcome {
            Some(JobOutcome::Completed { skipped }) => {
                assert_eq!(skipped.len(), 1, "exactly the skipped file is listed: {skipped:?}");
                assert_eq!(skipped[0].path, PathBuf::from("/v1/work/proj/shared/lib.dll"));
            }
            other => panic!("expected Completed with one skipped item, got {other:?}"),
        }
        assert_shared_target_intact(&fake);
        assert!(fake.metadata(Path::new("/v1/work/proj/shared")).unwrap().reparse.is_reparse_point(), "the junction survives an incomplete copy");
        assert!(fake.metadata(Path::new("/v1/work/proj")).is_ok(), "the directory holding the kept link survives");
        assert!(fake.metadata(Path::new("/v1/work/proj/file.txt")).is_err(), "the regular file was moved");
        assert_eq!(fake.metadata(Path::new("/v2/backup/proj/shared/x86/a.dll")).unwrap().size, 20, "the rest of the link's content was copied");
        assert!(fake.metadata(Path::new("/v2/backup/proj/shared/lib.dll")).is_err());
    }

    /// Design D3: a file symlink is copied by following it, and only the link
    /// is removed — the target file and its read-only attribute are never
    /// touched (`set_readonly` would follow the link).
    #[test]
    fn move_cross_volume_of_a_file_symlink_removes_only_the_link() {
        let fake = FakeFs::new();
        fake.set_volume(Path::new("/v1"), 1);
        fake.set_volume(Path::new("/v2"), 2);
        fake.add_dir(Path::new("/v1/data"));
        fake.add_file(Path::new("/v1/data/real.txt"), 42);
        fake.set_readonly(Path::new("/v1/data/real.txt"), true).unwrap();
        fake.add_dir(Path::new("/v1/work"));
        fake.add_reparse_point(Path::new("/v1/work/link.txt"), false, Path::new("/v1/data/real.txt"), super::super::fs::ReparseKind::Symlink);
        fake.add_dir(Path::new("/v2/backup"));

        let job = Job {
            kind: JobKind::Move,
            sources: vec![source("link.txt", "/v1/work/link.txt")],
            source_dir: PathBuf::from("/v1/work"),
            dest_dir: PathBuf::from("/v2/backup"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);

        assert!(matches!(&sink.outcome, Some(JobOutcome::Completed { skipped }) if skipped.is_empty()), "{:?}", sink.outcome);
        assert_eq!(fake.metadata(Path::new("/v2/backup/link.txt")).unwrap().size, 42, "the destination receives the target's content");
        assert!(fake.metadata(Path::new("/v1/work/link.txt")).is_err(), "the link is removed");
        let target = fake.metadata(Path::new("/v1/data/real.txt")).unwrap();
        assert_eq!(target.size, 42, "the target file survives");
        assert!(target.readonly, "the target's read-only attribute is untouched");
    }

    /// Design D3: a link that cannot be removed is reported in the summary,
    /// not silently ignored like a non-empty directory.
    #[test]
    fn move_cross_volume_reports_a_junction_that_could_not_be_removed() {
        let fake = junction_in_tree_fixture();
        fake.inject(InjectableOp::RemoveDir, Path::new("/v1/work/proj/shared"), io::ErrorKind::PermissionDenied);
        let mut sink = RecordingSink::new();
        run_job(&move_proj_to_backup(), &fake, &mut sink);

        match &sink.outcome {
            Some(JobOutcome::Completed { skipped }) => {
                assert_eq!(skipped.len(), 1, "{skipped:?}");
                assert_eq!(skipped[0].path, PathBuf::from("/v1/work/proj/shared"));
                assert!(skipped[0].reason.starts_with("copied, but the link could not be removed: "), "{}", skipped[0].reason);
            }
            other => panic!("expected Completed with one skipped item, got {other:?}"),
        }
        assert_shared_target_intact(&fake);
        assert!(fake.metadata(Path::new("/v1/work/proj/shared")).is_ok(), "the link is still there");
        assert_eq!(fake.metadata(Path::new("/v2/backup/proj/shared/lib.dll")).unwrap().size, 10, "its content was copied");
    }

    #[test]
    fn case_only_rename_succeeds_via_identity_check() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/d"));
        fake.add_file(Path::new("/d/Name.txt"), 4);
        let job = Job {
            kind: JobKind::Move,
            sources: vec![source("Name.txt", "/d/Name.txt")],
            source_dir: PathBuf::from("/d"),
            dest_dir: PathBuf::from("/d"),
            new_dir_name: None,
        };
        // Rename target: same directory, different case. The panel/TUI layer
        // would normally resolve this via a rename-in-place path, but the
        // job engine itself must not treat same-identity paths as conflicts.
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);
        // Renaming onto itself (case-only) is a same-identity no-op target;
        // the underlying `rename` still succeeds since FakeFs treats it as a
        // plain move.
        assert!(matches!(sink.outcome, Some(JobOutcome::Completed { .. })));
    }

    // -------------------------------------------------------------
    // JobKind::Rename (file-action-menu "In-place Rename")
    // -------------------------------------------------------------

    fn rename_job(dir: &str, old: &str, new: &str) -> Job {
        Job {
            kind: JobKind::Rename,
            sources: vec![source(old, &format!("{dir}/{old}"))],
            source_dir: PathBuf::from(dir),
            dest_dir: PathBuf::from(dir),
            new_dir_name: Some(OsString::from(new)),
        }
    }

    #[test]
    fn rename_renames_the_entry_in_place() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/d"));
        fake.add_file(Path::new("/d/draft.txt"), 4);
        let job = rename_job("/d", "draft.txt", "final.txt");
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);
        assert!(matches!(sink.outcome, Some(JobOutcome::Completed { skipped }) if skipped.is_empty()));
        assert!(fake.metadata(Path::new("/d/final.txt")).is_ok());
        assert!(fake.metadata(Path::new("/d/draft.txt")).is_err());
    }

    #[test]
    fn rename_case_only_change_succeeds_via_identity_check() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/d"));
        fake.add_file(Path::new("/d/readme.md"), 4);
        let job = rename_job("/d", "readme.md", "README.md");
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);
        assert!(matches!(sink.outcome, Some(JobOutcome::Completed { skipped }) if skipped.is_empty()));
        assert!(fake.metadata(Path::new("/d/README.md")).is_ok());
    }

    #[test]
    fn rename_onto_an_existing_different_file_asks_for_conflict_resolution() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/d"));
        fake.add_file(Path::new("/d/a.txt"), 4);
        fake.add_file(Path::new("/d/b.txt"), 9);
        let job = rename_job("/d", "a.txt", "b.txt");
        let mut sink = RecordingSink::new();
        sink.conflict_answers.push(ConflictChoice::Skip);
        run_job(&job, &fake, &mut sink);
        match sink.outcome {
            Some(JobOutcome::Completed { skipped }) => assert_eq!(skipped.len(), 1, "the conflict was surfaced and Skip was honored"),
            other => panic!("expected a Completed outcome with one skip, got {other:?}"),
        }
        // Declining (Skip) must leave both files untouched.
        assert!(fake.metadata(Path::new("/d/a.txt")).is_ok());
        assert_eq!(fake.metadata(Path::new("/d/b.txt")).unwrap().size, 9);
    }

    #[test]
    fn rename_overwrite_conflict_choice_replaces_the_target() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/d"));
        fake.add_file(Path::new("/d/a.txt"), 4);
        fake.add_file(Path::new("/d/b.txt"), 9);
        let job = rename_job("/d", "a.txt", "b.txt");
        let mut sink = RecordingSink::new();
        sink.conflict_answers.push(ConflictChoice::Overwrite);
        run_job(&job, &fake, &mut sink);
        assert!(matches!(sink.outcome, Some(JobOutcome::Completed { skipped }) if skipped.is_empty()));
        assert!(fake.metadata(Path::new("/d/a.txt")).is_err());
        assert_eq!(fake.metadata(Path::new("/d/b.txt")).unwrap().size, 4, "b.txt now holds a.txt's renamed content");
    }

    #[test]
    fn overwrite_all_latches_across_multiple_conflicts() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/src"));
        fake.add_file(Path::new("/src/a.txt"), 1);
        fake.add_file(Path::new("/src/b.txt"), 1);
        fake.add_dir(Path::new("/dst"));
        fake.add_file(Path::new("/dst/a.txt"), 99);
        fake.add_file(Path::new("/dst/b.txt"), 99);

        let job = Job {
            kind: JobKind::Copy,
            sources: vec![source("a.txt", "/src/a.txt"), source("b.txt", "/src/b.txt")],
            source_dir: PathBuf::from("/src"),
            dest_dir: PathBuf::from("/dst"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        sink.conflict_answers.push(ConflictChoice::OverwriteAll);
        run_job(&job, &fake, &mut sink);
        assert!(matches!(sink.outcome, Some(JobOutcome::Completed { .. })));
        assert_eq!(fake.metadata(Path::new("/dst/a.txt")).unwrap().size, 1);
        assert_eq!(fake.metadata(Path::new("/dst/b.txt")).unwrap().size, 1);
    }

    #[test]
    fn skip_all_latches_and_records_skipped_items() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/src"));
        fake.add_file(Path::new("/src/a.txt"), 1);
        fake.add_file(Path::new("/src/b.txt"), 1);
        fake.add_dir(Path::new("/dst"));
        fake.add_file(Path::new("/dst/a.txt"), 99);
        fake.add_file(Path::new("/dst/b.txt"), 99);

        let job = Job {
            kind: JobKind::Copy,
            sources: vec![source("a.txt", "/src/a.txt"), source("b.txt", "/src/b.txt")],
            source_dir: PathBuf::from("/src"),
            dest_dir: PathBuf::from("/dst"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        sink.conflict_answers.push(ConflictChoice::SkipAll);
        run_job(&job, &fake, &mut sink);
        match sink.outcome {
            Some(JobOutcome::Completed { skipped }) => assert_eq!(skipped.len(), 2),
            other => panic!("expected two skipped items, got {other:?}"),
        }
        assert_eq!(fake.metadata(Path::new("/dst/a.txt")).unwrap().size, 99, "left untouched");
    }

    // -----------------------------------------------------------------
    // process-lifecycle-hardening: Cancel from the conflict prompt
    // (operation-dialogs "Overwrite-conflict dialog" — "Esc cancels the
    // job from the conflict dialog"; design D7)
    // -----------------------------------------------------------------

    /// Three sources where only the second collides at the destination.
    fn three_file_job(kind: JobKind, fake: &FakeFs) -> Job {
        fake.add_dir(Path::new("/src"));
        fake.add_file(Path::new("/src/a.txt"), 1);
        fake.add_file(Path::new("/src/b.txt"), 2);
        fake.add_file(Path::new("/src/c.txt"), 3);
        fake.add_dir(Path::new("/dst"));
        fake.add_file(Path::new("/dst/b.txt"), 99);
        Job {
            kind,
            sources: vec![source("a.txt", "/src/a.txt"), source("b.txt", "/src/b.txt"), source("c.txt", "/src/c.txt")],
            source_dir: PathBuf::from("/src"),
            dest_dir: PathBuf::from("/dst"),
            new_dir_name: None,
        }
    }

    #[test]
    fn cancel_at_a_copy_conflict_stops_the_job_without_touching_or_recording_the_target() {
        let fake = FakeFs::new();
        // Distinct volumes so a Move would take the copy-then-delete path;
        // for Copy it changes nothing.
        let job = three_file_job(JobKind::Copy, &fake);
        let mut sink = RecordingSink::new();
        sink.conflict_answers.push(ConflictChoice::Cancel);
        run_job(&job, &fake, &mut sink);

        assert_eq!(sink.conflicts_seen, 1, "the second file's conflict was asked once");
        assert!(matches!(sink.outcome, Some(JobOutcome::Cancelled { ref skipped }) if skipped.is_empty()), "cancelled with no skipped item for the conflicting target: {:?}", sink.outcome);
        assert_eq!(fake.metadata(Path::new("/dst/a.txt")).unwrap().size, 1, "the first file was copied before the conflict");
        assert_eq!(fake.metadata(Path::new("/dst/b.txt")).unwrap().size, 99, "the conflicting target is untouched");
        assert!(fake.metadata(Path::new("/dst/c.txt")).is_err(), "nothing after the conflict is processed");
    }

    #[test]
    fn cancel_at_a_cross_volume_move_conflict_leaves_the_remaining_sources_in_place() {
        let fake = FakeFs::new();
        fake.set_volume(Path::new("/src"), 1);
        fake.set_volume(Path::new("/dst"), 2);
        let job = three_file_job(JobKind::Move, &fake);
        let mut sink = RecordingSink::new();
        sink.conflict_answers.push(ConflictChoice::Cancel);
        run_job(&job, &fake, &mut sink);

        assert!(matches!(sink.outcome, Some(JobOutcome::Cancelled { ref skipped }) if skipped.is_empty()), "{:?}", sink.outcome);
        assert!(fake.metadata(Path::new("/src/a.txt")).is_err(), "the first file was moved before the conflict");
        assert_eq!(fake.metadata(Path::new("/dst/b.txt")).unwrap().size, 99, "the conflicting target is untouched");
        assert!(fake.metadata(Path::new("/src/b.txt")).is_ok() && fake.metadata(Path::new("/src/c.txt")).is_ok(), "the conflicting source and the third file stay put");
        assert!(fake.metadata(Path::new("/dst/c.txt")).is_err());
    }

    #[test]
    fn cancel_at_a_same_volume_move_conflict_stops_the_job_without_touching_the_target() {
        let fake = FakeFs::new();
        let job = three_file_job(JobKind::Move, &fake);
        let mut sink = RecordingSink::new();
        sink.conflict_answers.push(ConflictChoice::Cancel);
        run_job(&job, &fake, &mut sink);

        assert_eq!(sink.conflicts_seen, 1);
        assert!(matches!(sink.outcome, Some(JobOutcome::Cancelled { ref skipped }) if skipped.is_empty()), "{:?}", sink.outcome);
        assert!(fake.metadata(Path::new("/dst/a.txt")).is_ok() && fake.metadata(Path::new("/src/a.txt")).is_err(), "the first file was moved");
        assert_eq!(fake.metadata(Path::new("/dst/b.txt")).unwrap().size, 99, "the conflicting target is untouched");
        assert!(fake.metadata(Path::new("/src/b.txt")).is_ok(), "the conflicting source stays");
        assert!(fake.metadata(Path::new("/src/c.txt")).is_ok() && fake.metadata(Path::new("/dst/c.txt")).is_err(), "the third file is not moved");
    }

    #[test]
    fn cancel_at_a_rename_conflict_ends_the_job_cancelled_with_both_files_untouched() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/d"));
        fake.add_file(Path::new("/d/a.txt"), 4);
        fake.add_file(Path::new("/d/b.txt"), 9);
        let job = rename_job("/d", "a.txt", "b.txt");
        let mut sink = RecordingSink::new();
        sink.conflict_answers.push(ConflictChoice::Cancel);
        run_job(&job, &fake, &mut sink);

        assert!(matches!(sink.outcome, Some(JobOutcome::Cancelled { ref skipped }) if skipped.is_empty()), "{:?}", sink.outcome);
        assert_eq!(fake.metadata(Path::new("/d/a.txt")).unwrap().size, 4);
        assert_eq!(fake.metadata(Path::new("/d/b.txt")).unwrap().size, 9, "the conflicting target is untouched");
    }

    #[test]
    fn error_retry_then_succeeds() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/panel"));
        fake.inject(super::super::fs::InjectableOp::CreateDir, Path::new("/panel/newdir"), io::ErrorKind::PermissionDenied);
        let job = Job {
            kind: JobKind::Mkdir,
            sources: vec![],
            source_dir: PathBuf::from("/panel"),
            dest_dir: PathBuf::from("/panel"),
            new_dir_name: Some(OsString::from("newdir")),
        };
        let mut sink = RecordingSink::new();
        sink.error_answers.push(ErrorChoice::Retry);
        run_job(&job, &fake, &mut sink);
        assert!(matches!(sink.outcome, Some(JobOutcome::Completed { .. })));
    }

    #[test]
    fn error_abort_stops_the_job() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/src"));
        fake.add_file(Path::new("/src/a.txt"), 1);
        fake.add_file(Path::new("/src/b.txt"), 1);
        fake.add_dir(Path::new("/dst"));
        fake.inject(super::super::fs::InjectableOp::CopyAfterChunks(0), Path::new("/dst/a.txt"), io::ErrorKind::PermissionDenied);

        let job = Job {
            kind: JobKind::Copy,
            sources: vec![source("a.txt", "/src/a.txt"), source("b.txt", "/src/b.txt")],
            source_dir: PathBuf::from("/src"),
            dest_dir: PathBuf::from("/dst"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        sink.error_answers.push(ErrorChoice::Abort);
        run_job(&job, &fake, &mut sink);
        assert!(matches!(sink.outcome, Some(JobOutcome::Cancelled { .. })));
        assert!(fake.metadata(Path::new("/dst/b.txt")).is_err(), "job stopped before reaching b.txt");
    }

    #[test]
    fn cancellation_mid_job_emits_cancelled_outcome() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/src"));
        fake.add_file(Path::new("/src/a.txt"), 1);
        fake.add_file(Path::new("/src/b.txt"), 1);
        fake.add_dir(Path::new("/dst"));

        let job = Job {
            kind: JobKind::Copy,
            sources: vec![source("a.txt", "/src/a.txt"), source("b.txt", "/src/b.txt")],
            source_dir: PathBuf::from("/src"),
            dest_dir: PathBuf::from("/dst"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        sink.cancel.cancel();
        run_job(&job, &fake, &mut sink);
        assert!(matches!(sink.outcome, Some(JobOutcome::Cancelled { .. })));
        assert!(fake.metadata(Path::new("/dst/a.txt")).is_err());
    }

    #[test]
    fn permission_denied_injection_surfaces_as_error_dialog() {
        let fake = FakeFs::new();
        fake.add_file(Path::new("/a.txt"), 1);
        fake.inject(super::super::fs::InjectableOp::RemoveFile, Path::new("/a.txt"), io::ErrorKind::PermissionDenied);
        let job = Job {
            kind: JobKind::Delete,
            sources: vec![source("a.txt", "/a.txt")],
            source_dir: PathBuf::from("/"),
            dest_dir: PathBuf::from("/"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        sink.error_answers.push(ErrorChoice::Skip);
        run_job(&job, &fake, &mut sink);
        match sink.outcome {
            Some(JobOutcome::Completed { skipped }) => assert_eq!(skipped.len(), 1),
            other => panic!("expected one skipped item, got {other:?}"),
        }
    }

    #[test]
    fn concurrent_conflict_replies_via_arc_mutex_sink_are_thread_safe() {
        // Sanity check that JobSink is object-safe and usable from a spawned
        // thread the way filecommand-tui's real worker will use it.
        let fake = Arc::new(FakeFs::new());
        fake.add_dir(Path::new("/src"));
        fake.add_file(Path::new("/src/a.txt"), 1);
        fake.add_dir(Path::new("/dst"));
        let job = Job {
            kind: JobKind::Copy,
            sources: vec![source("a.txt", "/src/a.txt")],
            source_dir: PathBuf::from("/src"),
            dest_dir: PathBuf::from("/dst"),
            new_dir_name: None,
        };
        let sink = Arc::new(Mutex::new(RecordingSink::new()));
        let fake2 = Arc::clone(&fake);
        let sink2 = Arc::clone(&sink);
        let handle = std::thread::spawn(move || {
            let mut guard = sink2.lock().unwrap();
            run_job(&job, fake2.as_ref(), &mut *guard);
        });
        handle.join().unwrap();
        assert!(matches!(sink.lock().unwrap().outcome, Some(JobOutcome::Completed { .. })));
    }

    // -------------------------------------------------------------
    // Copy-onto-itself guard (file-operations "A copy never
    // overwrites its own source")
    // -------------------------------------------------------------

    /// A file with a known size, an alternate stream, and a known last-write
    /// time. `FakeFs` does not model truncation, but a copy onto the same
    /// path replaces the node — dropping the stream and the timestamp — so
    /// those two are what detect a self-copy here.
    fn self_copy_fixture(fake: &FakeFs, path: &Path) -> SystemTime {
        fake.add_file_with_stream(path, 2048, "Zone.Identifier", 512);
        let modified = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        fake.set_modified(path, modified).unwrap();
        modified
    }

    fn assert_untouched(fake: &FakeFs, path: &Path, modified: SystemTime) {
        let md = fake.metadata(path).unwrap();
        assert_eq!(md.size, 2048, "content size must survive");
        assert_eq!(
            md.modified,
            Some(modified),
            "last-write time must survive (a self-copy would have replaced the node)"
        );
        assert_eq!(
            fake.list_streams(path).unwrap(),
            vec![OsString::from("Zone.Identifier")],
            "the alternate stream must survive"
        );
    }

    fn assert_skipped_same_file(sink: &RecordingSink, src: &str) {
        match &sink.outcome {
            Some(JobOutcome::Completed { skipped }) => {
                assert_eq!(
                    skipped.len(),
                    1,
                    "the same-file item is reported as skipped: {skipped:?}"
                );
                assert_eq!(skipped[0].path, PathBuf::from(src));
                assert!(
                    skipped[0].reason.contains("same file"),
                    "the reason should name the self-copy: {}",
                    skipped[0].reason
                );
            }
            other => panic!("expected Completed with one skipped item, got {other:?}"),
        }
        assert_eq!(
            sink.conflicts_seen, 0,
            "a same-file copy must not raise the overwrite-conflict dialog"
        );
    }

    #[test]
    fn copy_into_own_directory_skips_same_file_without_conflict() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/work"));
        let modified = self_copy_fixture(&fake, Path::new("/work/report.txt"));

        let job = Job {
            kind: JobKind::Copy,
            sources: vec![source("report.txt", "/work/report.txt")],
            source_dir: PathBuf::from("/work"),
            dest_dir: PathBuf::from("/work"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);

        assert_skipped_same_file(&sink, "/work/report.txt");
        assert_untouched(&fake, Path::new("/work/report.txt"), modified);
    }

    #[test]
    fn copy_through_junction_alias_onto_itself_is_skipped() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/work"));
        let modified = self_copy_fixture(&fake, Path::new("/work/report.txt"));
        fake.add_reparse_point(
            Path::new("/alias"),
            true,
            Path::new("/work"),
            super::super::fs::ReparseKind::Junction,
        );

        let job = Job {
            kind: JobKind::Copy,
            sources: vec![source("report.txt", "/work/report.txt")],
            source_dir: PathBuf::from("/work"),
            dest_dir: PathBuf::from("/alias"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);

        // `/alias/report.txt` resolves to `/work/report.txt` through the
        // junction, so the followed identities match and the item is skipped.
        assert_skipped_same_file(&sink, "/work/report.txt");
        assert_untouched(&fake, Path::new("/work/report.txt"), modified);
    }

    #[test]
    fn move_copy_then_delete_onto_same_file_keeps_the_source() {
        let fake = FakeFs::new();
        fake.set_volume(Path::new("/v1"), 1);
        fake.set_volume(Path::new("/v2"), 2);
        fake.add_dir(Path::new("/v1/work"));
        let modified = self_copy_fixture(&fake, Path::new("/v1/work/report.txt"));
        fake.add_reparse_point(
            Path::new("/v2/alias"),
            true,
            Path::new("/v1/work"),
            super::super::fs::ReparseKind::Junction,
        );

        // Different volume tags make `same_volume` answer false, so the move
        // takes the copy-then-delete path rather than the rename path.
        let job = Job {
            kind: JobKind::Move,
            sources: vec![source("report.txt", "/v1/work/report.txt")],
            source_dir: PathBuf::from("/v1/work"),
            dest_dir: PathBuf::from("/v2/alias"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);

        assert_skipped_same_file(&sink, "/v1/work/report.txt");
        assert_untouched(&fake, Path::new("/v1/work/report.txt"), modified);
    }

    /// Delegates every [`FileSystem`] call to the inner fake but reports no
    /// file identity, simulating a file whose handle cannot be opened for
    /// identity (design D1's unknown-identity path fallback).
    struct NoIdentity<F: FileSystem>(F);

    impl<F: FileSystem> FileSystem for NoIdentity<F> {
        fn metadata(&self, path: &Path) -> io::Result<FsMetadata> {
            let mut md = self.0.metadata(path)?;
            md.file_id = None;
            Ok(md)
        }
        fn metadata_follow(&self, path: &Path) -> io::Result<FsMetadata> {
            let mut md = self.0.metadata_follow(path)?;
            md.file_id = None;
            Ok(md)
        }
        fn read_dir(&self, path: &Path) -> io::Result<Vec<RawEntry>> {
            self.0.read_dir(path)
        }
        fn create_dir(&self, path: &Path) -> io::Result<()> {
            self.0.create_dir(path)
        }
        fn copy_file_chunked(
            &self,
            from: &Path,
            to: &Path,
            chunk_bytes: usize,
            on_chunk: &mut dyn FnMut(u64) -> ChunkControl,
        ) -> io::Result<u64> {
            self.0.copy_file_chunked(from, to, chunk_bytes, on_chunk)
        }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.0.rename(from, to)
        }
        fn remove_file(&self, path: &Path) -> io::Result<()> {
            self.0.remove_file(path)
        }
        fn remove_dir(&self, path: &Path) -> io::Result<()> {
            self.0.remove_dir(path)
        }
        fn set_readonly(&self, path: &Path, readonly: bool) -> io::Result<()> {
            self.0.set_readonly(path, readonly)
        }
        fn set_modified(&self, path: &Path, modified: SystemTime) -> io::Result<()> {
            self.0.set_modified(path, modified)
        }
        fn list_streams(&self, path: &Path) -> io::Result<Vec<OsString>> {
            self.0.list_streams(path)
        }
        fn copy_stream_chunked(
            &self,
            from: &Path,
            stream: &OsStr,
            to: &Path,
            chunk_bytes: usize,
        ) -> io::Result<u64> {
            self.0.copy_stream_chunked(from, stream, to, chunk_bytes)
        }
        fn supports_named_streams(&self, path: &Path) -> io::Result<bool> {
            self.0.supports_named_streams(path)
        }
        fn same_volume(&self, a: &Path, b: &Path) -> io::Result<bool> {
            self.0.same_volume(a, b)
        }
    }

    #[test]
    fn copy_into_own_directory_without_identity_uses_path_fallback() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/work"));
        let modified = self_copy_fixture(&fake, Path::new("/work/report.txt"));
        let fs = NoIdentity(fake);

        let job = Job {
            kind: JobKind::Copy,
            sources: vec![source("report.txt", "/work/report.txt")],
            source_dir: PathBuf::from("/work"),
            dest_dir: PathBuf::from("/work"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        run_job(&job, &fs, &mut sink);

        // With identity unreadable, `identity_matches` answers false and the
        // same file would raise an overwrite conflict against itself; the
        // path fallback must instead skip it without asking.
        assert_skipped_same_file(&sink, "/work/report.txt");
        assert_untouched(&fs.0, Path::new("/work/report.txt"), modified);
    }

    #[cfg(windows)]
    #[test]
    fn path_fallback_compares_case_insensitively_on_windows() {
        let mixed_case = Path::new(r"C:\Work\Report.txt");
        let normalized_dots = Path::new(r"c:\work\.\report.txt");
        let a = Path::new(r"C:\work\a.txt");
        let b = Path::new(r"C:\work\b.txt");
        assert!(normalized_paths_equal(mixed_case, normalized_dots));
        assert!(!normalized_paths_equal(a, b));
    }

    #[cfg(not(windows))]
    #[test]
    fn path_fallback_compares_exactly_off_windows() {
        let with_dot = Path::new("/work/./report.txt");
        let plain = Path::new("/work/report.txt");
        let different_case = Path::new("/work/Report.txt");
        assert!(normalized_paths_equal(with_dot, plain));
        assert!(!normalized_paths_equal(different_case, plain));
    }

    // -------------------------------------------------------------
    // error-diagnostics: metadata-restore warnings (task 5.4)
    // -------------------------------------------------------------

    /// A one-file copy job `/src/<name>` → `/dst`.
    fn copy_job_of(name: &str) -> Job {
        Job {
            kind: JobKind::Copy,
            sources: vec![source(name, &format!("/src/{name}"))],
            source_dir: PathBuf::from("/src"),
            dest_dir: PathBuf::from("/dst"),
            new_dir_name: None,
        }
    }

    /// The pinned source mtime that exercises the modification-time
    /// restore (a `FakeFs` file starts with `modified: None`, which skips
    /// the restore entirely).
    fn pin_modified(fake: &FakeFs, path: &Path) {
        fake.set_modified(path, SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000)).unwrap();
    }

    #[test]
    fn set_modified_failure_becomes_a_warning_and_the_file_still_counts() {
        // operation-dialogs "Metadata-restore failures are reported in the
        // end-of-job summary": "Timestamp restore failure becomes a
        // warning".
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/src"));
        fake.add_file(Path::new("/src/report.docx"), 10);
        pin_modified(&fake, Path::new("/src/report.docx"));
        fake.add_dir(Path::new("/dst"));
        fake.inject(InjectableOp::SetModified, Path::new("/dst/report.docx"), io::ErrorKind::PermissionDenied);

        let mut sink = RecordingSink::new();
        run_job(&copy_job_of("report.docx"), &fake, &mut sink);

        match &sink.outcome {
            Some(JobOutcome::Completed { skipped }) => {
                assert_eq!(skipped.len(), 1, "exactly one warning, and it is not a skip: {skipped:?}");
                assert_eq!(skipped[0].kind, ItemKind::Warning);
                assert_eq!(skipped[0].path, PathBuf::from("/dst/report.docx"), "the warning names the destination path");
                assert!(skipped[0].reason.starts_with("modification time not restored: "), "{}", skipped[0].reason);
            }
            other => panic!("expected Completed with one warning, got {other:?}"),
        }
        assert!(sink.events.iter().any(|e| e == "progress:1/1"), "the file counts toward completed files");
        assert!(fake.metadata(Path::new("/dst/report.docx")).is_ok(), "the destination content was written");
    }

    #[test]
    fn stream_copy_failure_becomes_a_warning_naming_the_stream() {
        // "Alternate data stream copy failure becomes a warning".
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/src"));
        fake.add_file_with_stream(Path::new("/src/photo.jpg"), 100, "Zone.Identifier", 24);
        fake.add_dir(Path::new("/dst"));
        fake.inject(InjectableOp::CopyStream, Path::new("/dst/photo.jpg"), io::ErrorKind::PermissionDenied);

        let mut sink = RecordingSink::new();
        run_job(&copy_job_of("photo.jpg"), &fake, &mut sink);

        match &sink.outcome {
            Some(JobOutcome::Completed { skipped }) => {
                assert_eq!(skipped.len(), 1, "one warning for the failed stream: {skipped:?}");
                assert_eq!(skipped[0].kind, ItemKind::Warning);
                assert_eq!(skipped[0].path, PathBuf::from("/dst/photo.jpg"));
                assert!(skipped[0].reason.starts_with("stream Zone.Identifier not copied: "), "names the stream: {}", skipped[0].reason);
            }
            other => panic!("expected Completed with one warning, got {other:?}"),
        }
        assert!(sink.events.iter().any(|e| e == "progress:1/1"));
        assert_eq!(fake.metadata(Path::new("/dst/photo.jpg")).unwrap().size, 100, "the main content copied");
    }

    #[test]
    fn stream_listing_failure_becomes_a_single_warning() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/src"));
        fake.add_file_with_stream(Path::new("/src/a.txt"), 10, "Zone.Identifier", 3);
        fake.add_dir(Path::new("/dst"));
        // The listing is taken on the *source*; the warning is recorded
        // against the destination (design D5).
        fake.inject(InjectableOp::ListStreams, Path::new("/src/a.txt"), io::ErrorKind::PermissionDenied);

        let mut sink = RecordingSink::new();
        run_job(&copy_job_of("a.txt"), &fake, &mut sink);

        match &sink.outcome {
            Some(JobOutcome::Completed { skipped }) => {
                assert_eq!(skipped.len(), 1, "one warning for the failed listing: {skipped:?}");
                assert_eq!(skipped[0].kind, ItemKind::Warning);
                assert_eq!(skipped[0].path, PathBuf::from("/dst/a.txt"));
                assert!(skipped[0].reason.starts_with("alternate data streams not listed: "), "{}", skipped[0].reason);
            }
            other => panic!("expected Completed with one warning, got {other:?}"),
        }
        assert!(sink.events.iter().any(|e| e == "progress:1/1"));
    }

    #[test]
    fn read_only_restore_failure_becomes_a_warning() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/src"));
        fake.add_file(Path::new("/src/ro.txt"), 4);
        fake.set_readonly(Path::new("/src/ro.txt"), true).unwrap();
        fake.add_dir(Path::new("/dst"));
        fake.inject(InjectableOp::SetReadonly, Path::new("/dst/ro.txt"), io::ErrorKind::PermissionDenied);

        let mut sink = RecordingSink::new();
        run_job(&copy_job_of("ro.txt"), &fake, &mut sink);

        match &sink.outcome {
            Some(JobOutcome::Completed { skipped }) => {
                assert_eq!(skipped.len(), 1, "one warning for the failed restore: {skipped:?}");
                assert_eq!(skipped[0].kind, ItemKind::Warning);
                assert_eq!(skipped[0].path, PathBuf::from("/dst/ro.txt"));
                assert!(skipped[0].reason.starts_with("read-only attribute not restored: "), "{}", skipped[0].reason);
            }
            other => panic!("expected Completed with one warning, got {other:?}"),
        }
        assert!(sink.events.iter().any(|e| e == "progress:1/1"));
    }

    #[test]
    fn a_destination_without_stream_support_yields_one_job_level_warning() {
        // "A destination without stream support yields one warning":
        // forty-files-on-FAT32, shrunk to three.
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/src"));
        for (name, size) in [("a.txt", 10u64), ("b.txt", 20), ("c.txt", 30)] {
            fake.add_file_with_stream(Path::new(&format!("/src/{name}")), size, "Zone.Identifier", 3);
        }
        fake.add_dir(Path::new("/dst"));
        fake.set_named_streams_supported(false);

        let job = Job {
            kind: JobKind::Copy,
            sources: vec![source("a.txt", "/src/a.txt"), source("b.txt", "/src/b.txt"), source("c.txt", "/src/c.txt")],
            source_dir: PathBuf::from("/src"),
            dest_dir: PathBuf::from("/dst"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::new();
        run_job(&job, &fake, &mut sink);

        match &sink.outcome {
            Some(JobOutcome::Completed { skipped }) => {
                assert_eq!(skipped.len(), 1, "one aggregated warning, not one per file: {skipped:?}");
                assert_eq!(skipped[0].kind, ItemKind::Warning);
                assert_eq!(skipped[0].path, PathBuf::from("/dst"), "the warning names the destination");
                assert_eq!(skipped[0].reason, "alternate data streams not supported; streams of 3 file(s) not copied");
            }
            other => panic!("expected Completed with one warning, got {other:?}"),
        }
        assert!(sink.events.iter().any(|e| e == "progress:3/3"), "all three files still count as copied");
        for name in ["a.txt", "b.txt", "c.txt"] {
            let dst = PathBuf::from(format!("/dst/{name}"));
            assert!(fake.metadata(&dst).is_ok(), "{name} was copied");
            assert!(fake.list_streams(&dst).unwrap().is_empty(), "no per-file stream copy was attempted for {name}");
        }
    }

    #[test]
    fn a_clean_copy_records_no_warnings_and_an_empty_skipped() {
        // "Clean job shows no summary": a copy whose metadata all restores
        // leaves the outcome's skipped vector empty.
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/src"));
        fake.add_file_with_stream(Path::new("/src/a.txt"), 10, "Zone.Identifier", 3);
        pin_modified(&fake, Path::new("/src/a.txt"));
        fake.add_dir(Path::new("/dst"));

        let mut sink = RecordingSink::new();
        run_job(&copy_job_of("a.txt"), &fake, &mut sink);

        assert!(
            matches!(&sink.outcome, Some(JobOutcome::Completed { skipped }) if skipped.is_empty()),
            "no warnings on a clean copy: {:?}",
            sink.outcome
        );
        assert_eq!(fake.list_streams(Path::new("/dst/a.txt")).unwrap(), vec![OsString::from("Zone.Identifier")], "the stream was carried over");
        assert!(fake.metadata(Path::new("/dst/a.txt")).unwrap().modified.is_some(), "the mtime was restored");
    }
}
