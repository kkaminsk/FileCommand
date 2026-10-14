//! Integration tests for the job engine (`fs_ops::worker::run_job`) against
//! the real filesystem through [`RealFs`]. Each test runs in its own
//! temporary directory that is removed on drop.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use filecommand_core::fs_ops::{
    run_job, CancelFlag, ConflictChoice, ConflictInfo, ErrorChoice, ErrorInfo, Job, JobKind,
    JobOutcome, JobSink, ProgressInfo, RealFs, SourceItem,
};

/// A per-test directory under `std::env::temp_dir()`, named with the process
/// id and the test name, removed when the guard drops.
struct TempDir(PathBuf);

impl TempDir {
    fn new(test_name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "filecommand-realfs-{}-{test_name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Read-only files block `remove_dir_all` on Windows, so the bit is
        // cleared first. Links are not descended into: the walk clears the
        // link's own attributes only, never its target's.
        clear_readonly_recursive(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn clear_readonly_recursive(path: &Path) {
    let Ok(md) = fs::symlink_metadata(path) else { return };
    if md.permissions().readonly() {
        let mut perms = md.permissions();
        // A per-test scratch tree that is removed right after this; the
        // lint's concern (world-writable Unix modes) does not apply.
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        let _ = fs::set_permissions(path, perms);
    }
    if md.is_dir() && !md.file_type().is_symlink() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                clear_readonly_recursive(&entry.path());
            }
        }
    }
}

/// Every entry name in `dir` that carries the worker's temporary-file suffix.
fn partial_files_in(dir: &Path) -> Vec<OsString> {
    fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.file_name()).filter(|n| n.to_string_lossy().contains(".~fc-partial")).collect())
        .unwrap_or_default()
}

/// A minimal [`JobSink`] that records the outcome and answers conflicts and
/// errors from scripted queues. The queues default to Overwrite and Skip, so
/// a test that unexpectedly raises a dialog takes a path that fails on its
/// assertions instead of hanging; every raised error is recorded, so a test
/// can assert that no error-recovery dialog was shown.
#[derive(Default)]
struct RecordingSink {
    conflicts_seen: usize,
    conflict_answers: Vec<ConflictChoice>,
    errors_seen: Vec<ErrorInfo>,
    error_answers: Vec<ErrorChoice>,
    outcome: Option<JobOutcome>,
    cancel: CancelFlag,
    progress_events: usize,
    /// Raise the cancel flag when the n-th progress event (1-based) arrives.
    /// The job's opening `starting` report is event 1, so the first in-copy
    /// report is event 2.
    cancel_on_progress_event: Option<usize>,
}

impl JobSink for RecordingSink {
    fn progress(&mut self, _info: ProgressInfo) {
        self.progress_events += 1;
        if self.cancel_on_progress_event == Some(self.progress_events) {
            self.cancel.cancel();
        }
    }
    fn conflict(&mut self, _info: ConflictInfo) -> ConflictChoice {
        self.conflicts_seen += 1;
        if self.conflict_answers.is_empty() {
            ConflictChoice::Overwrite
        } else {
            self.conflict_answers.remove(0)
        }
    }
    fn error(&mut self, info: ErrorInfo) -> ErrorChoice {
        self.errors_seen.push(info);
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

fn file_source(path: &Path) -> SourceItem {
    SourceItem { original_name: path.file_name().unwrap().to_os_string(), path: path.to_path_buf(), is_dir: false }
}

fn copy_job(src: &Path, dest_dir: &Path) -> Job {
    Job { kind: JobKind::Copy, sources: vec![file_source(src)], source_dir: src.parent().unwrap().to_path_buf(), dest_dir: dest_dir.to_path_buf(), new_dir_name: None }
}

fn move_job(src: &Path, dest_dir: &Path) -> Job {
    Job { kind: JobKind::Move, sources: vec![file_source(src)], source_dir: src.parent().unwrap().to_path_buf(), dest_dir: dest_dir.to_path_buf(), new_dir_name: None }
}

fn assert_completed_clean(sink: &RecordingSink) {
    assert!(sink.errors_seen.is_empty(), "no error dialog: {:?}", sink.errors_seen);
    assert!(matches!(&sink.outcome, Some(JobOutcome::Completed { skipped }) if skipped.is_empty()), "{:?}", sink.outcome);
}

/// A deterministic payload larger than one 256 KiB copy chunk, so a copy
/// reports progress more than once.
fn multi_chunk_content() -> Vec<u8> {
    (0..1_200_000u32).map(|i| (i % 253) as u8).collect()
}

// ---------------------------------------------------------------------
// durable-file-copy, cross-platform (filesystem-error-handling "The suite
// exercises the real implementation on temporary directories")
// ---------------------------------------------------------------------

#[test]
fn copy_round_trip_lands_content_with_no_partial_left() {
    let temp = TempDir::new("copy-round-trip");
    let src = temp.path().join("src");
    let dst = temp.path().join("dst");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&dst).unwrap();
    let big = multi_chunk_content();
    fs::write(src.join("big.bin"), &big).unwrap();
    fs::write(src.join("small.txt"), b"small").unwrap();
    fs::write(src.join("empty.txt"), b"").unwrap();

    let job = Job {
        kind: JobKind::Copy,
        sources: vec![file_source(&src.join("big.bin")), file_source(&src.join("small.txt")), file_source(&src.join("empty.txt"))],
        source_dir: src.clone(),
        dest_dir: dst.clone(),
        new_dir_name: None,
    };
    let mut sink = RecordingSink::default();
    run_job(&job, &RealFs::new(), &mut sink);

    assert_completed_clean(&sink);
    assert_eq!(fs::read(dst.join("big.bin")).unwrap(), big);
    assert_eq!(fs::read(dst.join("small.txt")).unwrap(), b"small");
    assert_eq!(fs::read(dst.join("empty.txt")).unwrap(), b"");
    assert!(partial_files_in(&dst).is_empty(), "{:?}", partial_files_in(&dst));
    assert_eq!(fs::read(src.join("big.bin")).unwrap(), big, "a copy leaves its source alone");
}

/// file-operations "Cancel removes the partial file".
#[test]
fn cancel_during_a_copy_leaves_no_target_and_no_partial() {
    let temp = TempDir::new("cancel-cleanup");
    let src = temp.path().join("src");
    let dst = temp.path().join("dst");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&dst).unwrap();
    fs::write(src.join("big.bin"), multi_chunk_content()).unwrap();

    let mut sink = RecordingSink { cancel_on_progress_event: Some(2), ..Default::default() };
    run_job(&copy_job(&src.join("big.bin"), &dst), &RealFs::new(), &mut sink);

    assert!(matches!(sink.outcome, Some(JobOutcome::Cancelled { .. })), "{:?}", sink.outcome);
    assert!(sink.errors_seen.is_empty(), "a cancel is not an error: {:?}", sink.errors_seen);
    assert!(!dst.join("big.bin").exists(), "no file under the target name");
    assert!(partial_files_in(&dst).is_empty(), "{:?}", partial_files_in(&dst));
}

/// file-operations "Read-only target is cleared before overwrite": the
/// finished copy is renamed over a read-only target after its bit is cleared.
#[test]
fn read_only_target_is_overwritten_on_an_overwrite_answer() {
    let temp = TempDir::new("read-only-target");
    let src = temp.path().join("src");
    let dst = temp.path().join("dst");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&dst).unwrap();
    fs::write(src.join("report.txt"), b"new content").unwrap();
    let target = dst.join("report.txt");
    fs::write(&target, b"old").unwrap();
    let mut perms = fs::metadata(&target).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&target, perms).unwrap();

    let mut sink = RecordingSink { conflict_answers: vec![ConflictChoice::Overwrite], ..Default::default() };
    run_job(&copy_job(&src.join("report.txt"), &dst), &RealFs::new(), &mut sink);

    assert_eq!(sink.conflicts_seen, 1, "the existing target raised the overwrite-conflict dialog");
    assert_completed_clean(&sink);
    assert_eq!(fs::read(&target).unwrap(), b"new content");
    assert!(!fs::metadata(&target).unwrap().permissions().readonly(), "the copy carries the source's (writable) state");
    assert!(partial_files_in(&dst).is_empty(), "{:?}", partial_files_in(&dst));
}

/// file-operations "An existing file with the temporary name is left alone".
#[test]
fn an_existing_file_with_the_temporary_name_is_left_alone() {
    let temp = TempDir::new("existing-partial-name");
    let src = temp.path().join("src");
    let dst = temp.path().join("dst");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&dst).unwrap();
    fs::write(src.join("report.txt"), b"the copy").unwrap();
    let leftover = dst.join("report.txt.~fc-partial");
    fs::write(&leftover, b"someone else's file").unwrap();

    let mut sink = RecordingSink::default();
    run_job(&copy_job(&src.join("report.txt"), &dst), &RealFs::new(), &mut sink);

    assert_completed_clean(&sink);
    assert_eq!(fs::read(dst.join("report.txt")).unwrap(), b"the copy");
    assert_eq!(fs::read(&leftover).unwrap(), b"someone else's file", "not modified or removed");
    assert_eq!(partial_files_in(&dst), vec![OsString::from("report.txt.~fc-partial")], "only the pre-existing file remains; the -2 temporary was renamed away");
}

/// file-operations "A copy never overwrites its own source": F5 into the
/// directory the file is already in leaves it unchanged — a self-copy used
/// to truncate the file to zero bytes before the first read.
#[test]
fn copy_into_own_directory_leaves_file_unchanged() {
    let temp = TempDir::new("copy-into-own-directory");
    let file = temp.path().join("report.txt");
    let content: Vec<u8> = (0..2048u32).map(|i| (i % 251) as u8).collect();
    fs::write(&file, &content).unwrap();

    let job = Job {
        kind: JobKind::Copy,
        sources: vec![SourceItem {
            original_name: OsString::from("report.txt"),
            path: file.clone(),
            is_dir: false,
        }],
        source_dir: temp.path().to_path_buf(),
        dest_dir: temp.path().to_path_buf(),
        new_dir_name: None,
    };
    let mut sink = RecordingSink::default();
    run_job(&job, &RealFs::new(), &mut sink);

    assert_eq!(
        fs::read(&file).unwrap(),
        content,
        "the file keeps its original 2,048 bytes"
    );
    assert_eq!(
        sink.conflicts_seen, 0,
        "a same-file copy must not raise the overwrite-conflict dialog"
    );
    match &sink.outcome {
        Some(JobOutcome::Completed { skipped }) => {
            assert_eq!(
                skipped.len(),
                1,
                "the same-file item is reported as skipped: {skipped:?}"
            );
            assert_eq!(skipped[0].path, file);
            assert!(
                skipped[0].reason.contains("same file"),
                "the reason should name the self-copy: {}",
                skipped[0].reason
            );
        }
        other => panic!("expected Completed with one skipped item, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// Windows-only: real NTFS junctions (move-reparse-point-safety design D6)
// ---------------------------------------------------------------------

#[cfg(windows)]
mod windows_only {
    use super::*;
    use std::io;

    use filecommand_core::fs_ops::{ChunkControl, CopyOptions, CopyReport, FileSystem, FsMetadata, RawEntry};

    /// Create a directory junction `link` → `target` with `cmd /C mklink /J`.
    /// Junctions need no privilege, unlike symlinks, so this runs on any
    /// developer machine or CI runner.
    pub fn mklink_junction(link: &Path, target: &Path) {
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .expect("cmd /C mklink /J spawns");
        assert!(status.success(), "mklink /J {} {} failed: {status}", link.display(), target.display());
        let md = fs::symlink_metadata(link).expect("the junction exists");
        assert!(md.file_type().is_symlink(), "mklink /J produced a reparse point");
    }

    /// Delegates every [`FileSystem`] call to the inner implementation but
    /// answers `same_volume = false`, so a move takes the real
    /// copy-then-delete path even though the test fixture lives on one
    /// volume (move-reparse-point-safety design D6). It also logs the
    /// renames and file removals it sees, so a test can check that a move
    /// landed its copy before it touched the source.
    pub struct CrossVolume<F: FileSystem> {
        pub inner: F,
        pub ops: std::sync::Mutex<Vec<Op>>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Op {
        Rename(PathBuf, PathBuf),
        RemoveFile(PathBuf),
    }

    impl<F: FileSystem> CrossVolume<F> {
        pub fn new(inner: F) -> Self {
            CrossVolume { inner, ops: std::sync::Mutex::new(vec![]) }
        }
        pub fn ops(&self) -> Vec<Op> {
            self.ops.lock().unwrap().clone()
        }
    }

    impl<F: FileSystem> FileSystem for CrossVolume<F> {
        fn metadata(&self, path: &Path) -> io::Result<FsMetadata> {
            self.inner.metadata(path)
        }
        fn metadata_follow(&self, path: &Path) -> io::Result<FsMetadata> {
            self.inner.metadata_follow(path)
        }
        fn read_dir(&self, path: &Path) -> io::Result<Vec<RawEntry>> {
            self.inner.read_dir(path)
        }
        fn create_dir(&self, path: &Path) -> io::Result<()> {
            self.inner.create_dir(path)
        }
        fn copy_file(
            &self,
            from: &Path,
            to: &Path,
            opts: &CopyOptions,
            on_progress: &mut dyn FnMut(u64) -> ChunkControl,
        ) -> io::Result<CopyReport> {
            self.inner.copy_file(from, to, opts, on_progress)
        }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.ops.lock().unwrap().push(Op::Rename(from.to_path_buf(), to.to_path_buf()));
            self.inner.rename(from, to)
        }
        fn remove_file(&self, path: &Path) -> io::Result<()> {
            self.ops.lock().unwrap().push(Op::RemoveFile(path.to_path_buf()));
            self.inner.remove_file(path)
        }
        fn remove_dir(&self, path: &Path) -> io::Result<()> {
            self.inner.remove_dir(path)
        }
        fn set_readonly(&self, path: &Path, readonly: bool) -> io::Result<()> {
            self.inner.set_readonly(path, readonly)
        }
        fn supports_named_streams(&self, path: &Path) -> io::Result<bool> {
            self.inner.supports_named_streams(path)
        }
        fn same_volume(&self, _a: &Path, _b: &Path) -> io::Result<bool> {
            Ok(false)
        }
    }

    /// `src\proj\file.txt`, `outside\keep.txt`, `outside\x86\a.dll`, and the
    /// junction `src\proj\shared` → `outside`; an empty `dst`.
    struct JunctionFixture {
        src: PathBuf,
        dst: PathBuf,
        outside: PathBuf,
    }

    fn junction_fixture(temp: &TempDir) -> JunctionFixture {
        let src = temp.path().join("src");
        let dst = temp.path().join("dst");
        let outside = temp.path().join("outside");
        fs::create_dir_all(src.join("proj")).unwrap();
        fs::create_dir_all(outside.join("x86")).unwrap();
        fs::create_dir_all(&dst).unwrap();
        fs::write(src.join("proj").join("file.txt"), b"proj").unwrap();
        fs::write(outside.join("keep.txt"), b"keep me").unwrap();
        fs::write(outside.join("x86").join("a.dll"), b"a.dll").unwrap();
        mklink_junction(&src.join("proj").join("shared"), &outside);
        assert_eq!(fs::read(src.join("proj").join("shared").join("keep.txt")).unwrap(), b"keep me", "the junction resolves");
        JunctionFixture { src, dst, outside }
    }

    /// A plain file copies with nothing unrestored. The unnamed default
    /// stream (`::$DATA`) is not an alternate data stream: listing it as one
    /// once made every real-file copy record a "stream  not copied" warning,
    /// which the junction move test below would have tripped over.
    #[test]
    fn copy_of_a_plain_file_reports_nothing_unrestored() {
        let temp = TempDir::new("copy-plain-file");
        let file = temp.path().join("plain.txt");
        fs::write(&file, b"plain").unwrap();
        let report = RealFs::new()
            .copy_file(&file, &temp.path().join("copy.txt"), &CopyOptions { chunk_bytes: 4096, flush: false }, &mut |_| ChunkControl::Continue)
            .unwrap();
        assert_eq!(report.bytes, 5);
        assert!(report.unrestored.is_empty(), "{:?}", report.unrestored);
        assert_eq!(fs::read(temp.path().join("copy.txt")).unwrap(), b"plain");
    }

    /// Audit finding R-01 on real NTFS: file-operations "Cross-volume move
    /// of a tree containing a junction to an outside directory leaves that
    /// directory's files intact".
    #[test]
    fn move_of_tree_with_junction_to_outside_dir_keeps_outside_files() {
        let temp = TempDir::new("move-tree-with-junction");
        let fx = junction_fixture(&temp);

        let job = Job {
            kind: JobKind::Move,
            sources: vec![SourceItem { original_name: OsString::from("proj"), path: fx.src.join("proj"), is_dir: true }],
            source_dir: fx.src.clone(),
            dest_dir: fx.dst.clone(),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::default();
        run_job(&job, &CrossVolume::new(RealFs::new()), &mut sink);

        assert_completed_clean(&sink);
        assert_eq!(fs::read(fx.outside.join("keep.txt")).unwrap(), b"keep me", "R-01: the junction target's file survives the move");
        assert_eq!(fs::read(fx.outside.join("x86").join("a.dll")).unwrap(), b"a.dll", "R-01: the target's subdirectory survives");
        assert_eq!(fs::read(fx.dst.join("proj").join("file.txt")).unwrap(), b"proj");
        assert_eq!(fs::read(fx.dst.join("proj").join("shared").join("keep.txt")).unwrap(), b"keep me", "the destination holds a copy of the target's content");
        assert_eq!(fs::read(fx.dst.join("proj").join("shared").join("x86").join("a.dll")).unwrap(), b"a.dll");
        assert!(!fx.dst.join("proj").join("shared").symlink_metadata().unwrap().file_type().is_symlink(), "the destination `shared` is a plain directory, not a link");
        assert!(fx.src.join("proj").symlink_metadata().is_err(), "the source tree, including the junction, is removed");
    }

    /// Design D4; file-operations "Delete removes the link, not the target":
    /// on real NTFS a junction is a directory object, so it must be removed
    /// with `RemoveDirectoryW` — `DeleteFileW` answers "Access is denied".
    #[test]
    fn delete_of_junction_removes_link_and_keeps_target() {
        let temp = TempDir::new("delete-junction");
        let fx = junction_fixture(&temp);
        let link = fx.src.join("proj").join("shared");

        let job = Job {
            kind: JobKind::Delete,
            sources: vec![SourceItem { original_name: OsString::from("shared"), path: link.clone(), is_dir: true }],
            source_dir: fx.src.join("proj"),
            dest_dir: fx.src.join("proj"),
            new_dir_name: None,
        };
        let mut sink = RecordingSink::default();
        run_job(&job, &RealFs::new(), &mut sink);

        assert_completed_clean(&sink);
        assert!(link.symlink_metadata().is_err(), "the link is gone");
        assert_eq!(fs::read(fx.outside.join("keep.txt")).unwrap(), b"keep me", "the target is intact");
        assert_eq!(fs::read(fx.outside.join("x86").join("a.dll")).unwrap(), b"a.dll");
        assert_eq!(fs::read(fx.src.join("proj").join("file.txt")).unwrap(), b"proj", "the sibling file is untouched");
    }

    // -----------------------------------------------------------------
    // durable-file-copy, Windows/NTFS (design D9)
    // -----------------------------------------------------------------

    use std::fs::FileTimes;
    use std::os::windows::fs::{FileTimesExt, MetadataExt, OpenOptionsExt};
    use std::time::{Duration, SystemTime};

    const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    const FILE_ATTRIBUTE_ARCHIVE: u32 = 0x20;
    const ALL_FOUR: u32 = FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM | FILE_ATTRIBUTE_ARCHIVE;

    /// 2001-02-03 04:05:00 UTC and 2002-03-04 05:06:00 UTC, the spec's
    /// example times (file-operations "Creation time survives a copy").
    const CREATED_SECS: u64 = 981_173_100;
    const MODIFIED_SECS: u64 = 1_015_218_360;

    struct Decorated {
        created: SystemTime,
        modified: SystemTime,
        content: &'static [u8],
        stream: &'static [u8],
    }

    /// `path` with content, a `Zone.Identifier` stream, pinned creation and
    /// last-write times, and read-only + hidden + system + archive set with
    /// `attrib`, in that order (the attributes last, since read-only would
    /// block the writes).
    fn decorate(path: &Path) -> Decorated {
        let d = Decorated {
            created: SystemTime::UNIX_EPOCH + Duration::from_secs(CREATED_SECS),
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(MODIFIED_SECS),
            content: b"decorated content",
            stream: b"[ZoneTransfer]\r\nZoneId=3\r\n",
        };
        fs::write(path, d.content).unwrap();
        let mut stream_path = path.as_os_str().to_os_string();
        stream_path.push(":Zone.Identifier");
        fs::write(&stream_path, d.stream).unwrap();
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_times(FileTimes::new().set_created(d.created).set_modified(d.modified)).unwrap();
        drop(file);
        let status = std::process::Command::new("attrib").args(["+r", "+h", "+s", "+a"]).arg(path).status().expect("attrib runs");
        assert!(status.success(), "attrib failed: {status}");
        assert_decorated(path, &d);
        d
    }

    fn assert_decorated(path: &Path, d: &Decorated) {
        let md = fs::symlink_metadata(path).unwrap();
        assert_eq!(md.file_attributes() & ALL_FOUR, ALL_FOUR, "read-only, hidden, system and archive on {}: {:#x}", path.display(), md.file_attributes());
        assert_eq!(md.created().unwrap(), d.created, "creation time on {}", path.display());
        assert_eq!(md.modified().unwrap(), d.modified, "last-write time on {}", path.display());
        assert_eq!(fs::read(path).unwrap(), d.content);
        let mut stream_path = path.as_os_str().to_os_string();
        stream_path.push(":Zone.Identifier");
        assert_eq!(fs::read(&stream_path).unwrap(), d.stream, "the alternate data stream on {}", path.display());
    }

    /// file-operations "Alternate data streams and metadata survive a copy",
    /// "Hidden and system attributes survive a copy", "Creation time survives
    /// a copy".
    #[test]
    fn ads_attributes_and_timestamps_survive_a_copy() {
        let temp = TempDir::new("metadata-copy");
        let src = temp.path().join("src");
        let dst = temp.path().join("dst");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dst).unwrap();
        let d = decorate(&src.join("f.txt"));

        let mut sink = RecordingSink::default();
        run_job(&copy_job(&src.join("f.txt"), &dst), &RealFs::new(), &mut sink);

        assert_completed_clean(&sink);
        assert_decorated(&dst.join("f.txt"), &d);
        assert_decorated(&src.join("f.txt"), &d);
        assert!(partial_files_in(&dst).is_empty(), "{:?}", partial_files_in(&dst));
    }

    /// file-operations "A moved file keeps its metadata": the move's flush
    /// path clears and restores read-only around the flush.
    #[test]
    fn ads_attributes_and_timestamps_survive_a_cross_volume_move() {
        let temp = TempDir::new("metadata-move");
        let src = temp.path().join("src");
        let dst = temp.path().join("dst");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dst).unwrap();
        let d = decorate(&src.join("f.txt"));

        let mut sink = RecordingSink::default();
        run_job(&move_job(&src.join("f.txt"), &dst), &CrossVolume::new(RealFs::new()), &mut sink);

        assert_completed_clean(&sink);
        assert_decorated(&dst.join("f.txt"), &d);
        assert!(fs::symlink_metadata(src.join("f.txt")).is_err(), "the source was removed after the verified copy");
        assert!(partial_files_in(&dst).is_empty(), "{:?}", partial_files_in(&dst));
    }

    /// file-operations "A failed overwrite keeps the existing target": the
    /// source is held open with no sharing, so `CopyFileExW` fails with a
    /// sharing violation before a byte reaches the target's name.
    #[test]
    fn a_failed_overwrite_keeps_the_old_target_and_leaves_no_partial() {
        let temp = TempDir::new("failed-overwrite");
        let src = temp.path().join("src");
        let dst = temp.path().join("dst");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dst).unwrap();
        fs::write(src.join("report.txt"), b"new content").unwrap();
        fs::write(dst.join("report.txt"), b"old, complete content").unwrap();
        let _hold = fs::OpenOptions::new().read(true).share_mode(0).open(src.join("report.txt")).unwrap();

        let mut sink = RecordingSink { conflict_answers: vec![ConflictChoice::Overwrite], error_answers: vec![ErrorChoice::Skip], ..Default::default() };
        run_job(&copy_job(&src.join("report.txt"), &dst), &RealFs::new(), &mut sink);

        assert_eq!(sink.conflicts_seen, 1);
        assert_eq!(sink.errors_seen.len(), 1, "the sharing violation reached the error dialog: {:?}", sink.errors_seen);
        assert_eq!(fs::read(dst.join("report.txt")).unwrap(), b"old, complete content", "the target survives a failed overwrite");
        assert!(partial_files_in(&dst).is_empty(), "{:?}", partial_files_in(&dst));
        match &sink.outcome {
            Some(JobOutcome::Completed { skipped }) => {
                assert_eq!(skipped.len(), 1, "{skipped:?}");
                assert_eq!(skipped[0].path, src.join("report.txt"));
            }
            other => panic!("expected Completed with one skip, got {other:?}"),
        }
    }

    /// file-operations "Operation on a path longer than the legacy limit
    /// succeeds": the fixture is built under `\\?\`; the jobs get plain
    /// absolute paths, so the engine's own prefixing is what makes them work.
    #[test]
    fn long_paths_copy_move_and_delete() {
        let temp = TempDir::new("long-paths");
        let deep = |root: &Path| root.join("a".repeat(120)).join("b".repeat(120));
        let src = deep(&temp.path().join("src"));
        let dst = deep(&temp.path().join("dst"));
        let verbatim = |p: &Path| PathBuf::from(format!(r"\\?\{}", p.display()));
        assert!(src.as_os_str().len() > 260 && dst.as_os_str().len() > 260, "{}", src.display());
        fs::create_dir_all(verbatim(&src)).unwrap();
        fs::create_dir_all(verbatim(&dst)).unwrap();
        fs::write(verbatim(&src.join("long.txt")), b"far away").unwrap();
        fs::write(verbatim(&src.join("moved.txt")), b"moving far away").unwrap();

        let mut sink = RecordingSink::default();
        run_job(&copy_job(&src.join("long.txt"), &dst), &RealFs::new(), &mut sink);
        assert_completed_clean(&sink);
        assert_eq!(fs::read(verbatim(&dst.join("long.txt"))).unwrap(), b"far away");

        let mut sink = RecordingSink::default();
        run_job(&move_job(&src.join("moved.txt"), &dst), &CrossVolume::new(RealFs::new()), &mut sink);
        assert_completed_clean(&sink);
        assert_eq!(fs::read(verbatim(&dst.join("moved.txt"))).unwrap(), b"moving far away");
        assert!(fs::symlink_metadata(verbatim(&src.join("moved.txt"))).is_err());

        let job = Job { kind: JobKind::Delete, sources: vec![file_source(&dst.join("long.txt"))], source_dir: dst.clone(), dest_dir: dst.clone(), new_dir_name: None };
        let mut sink = RecordingSink::default();
        run_job(&job, &RealFs::new(), &mut sink);
        assert_completed_clean(&sink);
        assert!(fs::symlink_metadata(verbatim(&dst.join("long.txt"))).is_err());
        assert!(partial_files_in(&verbatim(&dst)).is_empty());
    }

    /// file-operations "Case-only rename succeeds".
    #[test]
    fn case_only_rename_succeeds_without_a_conflict() {
        let temp = TempDir::new("case-only-rename");
        let dir = temp.path().join("d");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("foo.txt"), b"foo").unwrap();

        let job = Job { kind: JobKind::Rename, sources: vec![file_source(&dir.join("foo.txt"))], source_dir: dir.clone(), dest_dir: dir.clone(), new_dir_name: Some(OsString::from("Foo.txt")) };
        let mut sink = RecordingSink::default();
        run_job(&job, &RealFs::new(), &mut sink);

        assert_eq!(sink.conflicts_seen, 0, "a case-only rename is not an overwrite conflict");
        assert_completed_clean(&sink);
        let names: Vec<OsString> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, vec![OsString::from("Foo.txt")]);
        assert_eq!(fs::read(dir.join("Foo.txt")).unwrap(), b"foo");
    }

    /// file-operations "Cross-volume move copies then deletes the source":
    /// the source removal is only attempted once the copy was renamed into
    /// place, and the destination then holds the full content.
    #[test]
    fn cross_volume_move_removes_the_source_only_after_the_copy_landed() {
        let temp = TempDir::new("cross-volume-move");
        let src = temp.path().join("src");
        let dst = temp.path().join("dst");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dst).unwrap();
        let big = multi_chunk_content();
        fs::write(src.join("big.bin"), &big).unwrap();

        let fs_ = CrossVolume::new(RealFs::new());
        let mut sink = RecordingSink::default();
        run_job(&move_job(&src.join("big.bin"), &dst), &fs_, &mut sink);

        assert_completed_clean(&sink);
        assert_eq!(fs::read(dst.join("big.bin")).unwrap(), big, "the destination holds the full content");
        assert!(fs::symlink_metadata(src.join("big.bin")).is_err(), "the source is gone");
        assert!(partial_files_in(&dst).is_empty(), "{:?}", partial_files_in(&dst));
        let ops = fs_.ops();
        let landed = ops.iter().position(|o| matches!(o, Op::Rename(_, to) if to == &dst.join("big.bin"))).expect("the copy was renamed over the target");
        let removed = ops.iter().position(|o| *o == Op::RemoveFile(src.join("big.bin"))).expect("the source was removed");
        assert!(landed < removed, "copy landed before the source was touched: {ops:?}");
    }

    // 5.8: the junction-in-a-moved-tree test shared with
    // move-reparse-point-safety is `move_of_tree_with_junction_to_outside_dir_keeps_outside_files`
    // above; it runs the same `CrossVolume<RealFs>` move through `CopyFileExW`.
}
