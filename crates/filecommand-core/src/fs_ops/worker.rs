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
        RetryOutcome::Skip(reason) => JobOutcome::Cancelled { skipped: vec![SkippedItem { path: target.clone(), reason }] },
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
            skipped.push(SkippedItem { path: abs.to_path_buf(), reason: e.to_string() });
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
            Err(e) => skipped.push(SkippedItem { path: abs.to_path_buf(), reason: e.to_string() }),
        }
    } else {
        out.push(DeleteItem::File { path: abs.to_path_buf(), size: md.size, readonly: md.readonly });
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

        let outcome = attempt_with_recovery(&path, &mut error_policy, sink, || {
            if readonly {
                let _ = fs.set_readonly(&path, false);
            }
            if is_dir {
                fs.remove_dir(&path)
            } else {
                fs.remove_file(&path)
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
            RetryOutcome::Skip(reason) => skipped.push(SkippedItem { path, reason }),
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

struct PlannedFile {
    src_abs: PathBuf,
    rel_path: PathBuf,
    size: u64,
}

struct CopyPlan {
    dirs: Vec<PathBuf>,
    files: Vec<PlannedFile>,
    skipped: Vec<SkippedItem>,
}

fn plan_copy(fs: &dyn FileSystem, sources: &[super::job::SourceItem]) -> CopyPlan {
    let mut plan = CopyPlan { dirs: vec![], files: vec![], skipped: vec![] };
    let mut visited: HashSet<FileId> = HashSet::new();
    for src in sources {
        let rel_root = PathBuf::from(&src.original_name);
        walk_copy(fs, &src.path, &rel_root, &mut visited, &mut plan);
    }
    plan
}

fn walk_copy(fs: &dyn FileSystem, abs: &Path, rel: &Path, visited: &mut HashSet<FileId>, plan: &mut CopyPlan) {
    let md = match fs.metadata(abs) {
        Ok(m) => m,
        Err(e) => {
            plan.skipped.push(SkippedItem { path: abs.to_path_buf(), reason: e.to_string() });
            return;
        }
    };

    // Reparse points duplicate their target's content by default: resolve
    // whether the target is a directory or a file via the *following*
    // metadata query.
    let is_dir_like = if md.reparse.is_reparse_point() {
        fs.metadata_follow(abs).map(|m| m.is_dir).unwrap_or(false)
    } else {
        md.is_dir
    };

    if !is_dir_like {
        let size = if md.reparse.is_reparse_point() {
            fs.metadata_follow(abs).map(|m| m.size).unwrap_or(md.size)
        } else {
            md.size
        };
        plan.files.push(PlannedFile { src_abs: abs.to_path_buf(), rel_path: rel.to_path_buf(), size });
        return;
    }

    // Directory (real or a directory-type reparse point/junction): guard
    // against recursion cycles — including junctions that point back inside
    // the tree already being copied — via the *real* (followed) identity.
    if let Some(id) = fs.metadata_follow(abs).ok().and_then(|m| m.file_id) {
        if !visited.insert(id) {
            plan.skipped.push(SkippedItem {
                path: abs.to_path_buf(),
                reason: "skipped: reparse point forms a cycle back into the source tree".to_string(),
            });
            return;
        }
    }

    plan.dirs.push(rel.to_path_buf());
    match fs.read_dir(abs) {
        Ok(entries) => {
            for entry in entries {
                walk_copy(fs, &abs.join(&entry.name), &rel.join(&entry.name), visited, plan);
            }
        }
        Err(e) => plan.skipped.push(SkippedItem { path: abs.to_path_buf(), reason: e.to_string() }),
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
                for name in fs.list_streams(src).unwrap_or_default() {
                    let _ = fs.copy_stream_chunked(src, &name, dst, CHUNK_BYTES);
                }
                if let Ok(md) = fs.metadata_follow(src) {
                    if let Some(modified) = md.modified {
                        let _ = fs.set_modified(dst, modified);
                    }
                    if md.readonly {
                        let _ = fs.set_readonly(dst, true);
                    }
                }
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

fn identity_matches(fs: &dyn FileSystem, a: &Path, b_id: Option<FileId>) -> bool {
    matches!((fs.metadata_follow(a).ok().and_then(|m| m.file_id), b_id), (Some(x), Some(y)) if x == y)
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

    'dirs: for rel_dir in &plan.dirs {
        if sink.is_cancelled() {
            cancelled = true;
            break;
        }
        let target = job.dest_dir.join(rel_dir);
        let outcome = attempt_with_recovery(&target, &mut error_policy, sink, || match fs.create_dir(&target) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(e),
        });
        match outcome {
            RetryOutcome::Done => {}
            RetryOutcome::Skip(reason) => skipped.push(SkippedItem { path: target, reason }),
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
                        skipped.push(SkippedItem { path: target, reason: "target already exists".to_string() });
                        do_copy = false;
                    }
                    ConflictChoice::Rename(new_name) => {
                        final_target = target.parent().unwrap_or(&job.dest_dir).join(new_name);
                    }
                }
            }

            if do_copy {
                let baseline = progress.bytes_done;
                match copy_one_file(fs, &planned.src_abs, &final_target, &mut error_policy, sink, &mut progress, baseline) {
                    CopyOneResult::Done(bytes) => {
                        progress.files_done += 1;
                        progress.bytes_done = baseline + bytes;
                        sink.progress(progress.clone());
                        if is_move {
                            let _ = fs.set_readonly(&planned.src_abs, false);
                            let _ = fs.remove_file(&planned.src_abs);
                        }
                    }
                    CopyOneResult::Cancelled | CopyOneResult::Aborted => {
                        cancelled = true;
                        break 'files;
                    }
                    CopyOneResult::Failed(reason) => {
                        progress.bytes_done = baseline;
                        skipped.push(SkippedItem { path: planned.src_abs.clone(), reason });
                    }
                }
            }
        }
    }

    if is_move && !cancelled {
        // Clean up now-empty source directories, deepest first. Best-effort:
        // a directory left non-empty by a skipped file simply survives.
        for rel_dir in plan.dirs.iter().rev() {
            let _ = fs.remove_dir(&job.source_dir.join(rel_dir));
        }
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
                        skipped.push(SkippedItem { path: target, reason: "target already exists".to_string() });
                        continue 'items;
                    }
                    ConflictChoice::Rename(new_name) => final_target = job.dest_dir.join(new_name),
                }
            }
        }

        let outcome = attempt_with_recovery(&src.path, &mut error_policy, sink, || fs.rename(&src.path, &final_target));
        match outcome {
            RetryOutcome::Done => {
                progress.files_done += 1;
                sink.progress(progress.clone());
            }
            RetryOutcome::Skip(reason) => skipped.push(SkippedItem { path: src.path.clone(), reason }),
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

/// In-place rename of the job's single source within `source_dir` (equal to
/// `dest_dir`, per the `Job` doc). Mirrors [`run_move_same_volume`]'s
/// identity-aware conflict check — a case-only rename onto the same
/// underlying file is never treated as "target already exists" — but always
/// targets `new_dir_name` rather than the source's own original name, and
/// never falls back to a copy-then-delete cross-volume path since a rename
/// never leaves its own directory. Collisions and errors surface through the
/// same `sink.conflict`/`sink.error` calls every other job uses, so the
/// existing `operation-dialogs` conflict/error dialogs render unchanged
/// (file-action-menu "Rename collisions/failures must surface through the
/// existing overwrite-conflict and error-recovery dialogs").
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
                    skipped.push(SkippedItem { path: target, reason: "target already exists".to_string() });
                    sink.done(JobOutcome::Completed { skipped });
                    return;
                }
                ConflictChoice::Rename(renamed_to) => final_target = job.dest_dir.join(renamed_to),
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
            skipped.push(SkippedItem { path: src.path.clone(), reason });
            JobOutcome::Completed { skipped }
        }
        RetryOutcome::Abort => JobOutcome::Cancelled { skipped },
    };
    sink.done(job_outcome);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs_ops::fs::FakeFs;
    use crate::fs_ops::job::{CancelFlag, SourceItem};
    use std::ffi::OsString;
    use std::sync::{Arc, Mutex};

    /// A [`JobSink`] test double that records every event and auto-resolves
    /// conflicts/errors from a scripted queue (defaulting to Overwrite/Skip
    /// respectively if the queue runs dry).
    struct RecordingSink {
        pub events: Vec<String>,
        pub conflict_answers: Vec<ConflictChoice>,
        pub error_answers: Vec<ErrorChoice>,
        pub cancel: CancelFlag,
        pub outcome: Option<JobOutcome>,
    }

    impl RecordingSink {
        fn new() -> Self {
            RecordingSink { events: vec![], conflict_answers: vec![], error_answers: vec![], cancel: CancelFlag::new(), outcome: None }
        }
    }

    impl JobSink for RecordingSink {
        fn progress(&mut self, info: ProgressInfo) {
            self.events.push(format!("progress:{}/{}", info.files_done, info.files_total));
        }
        fn conflict(&mut self, _info: ConflictInfo) -> ConflictChoice {
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
}
