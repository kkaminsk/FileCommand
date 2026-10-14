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
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A minimal [`JobSink`] that records the outcome and answers conflicts with
/// Overwrite, so a test that unexpectedly raises the overwrite-conflict
/// dialog takes the destructive path and fails on its assertions instead of
/// hanging. Errors are answered with Skip and recorded, so a test can assert
/// that no error-recovery dialog was raised.
#[derive(Default)]
struct RecordingSink {
    conflicts_seen: usize,
    errors_seen: Vec<ErrorInfo>,
    outcome: Option<JobOutcome>,
    cancel: CancelFlag,
}

impl JobSink for RecordingSink {
    fn progress(&mut self, _info: ProgressInfo) {}
    fn conflict(&mut self, _info: ConflictInfo) -> ConflictChoice {
        self.conflicts_seen += 1;
        ConflictChoice::Overwrite
    }
    fn error(&mut self, info: ErrorInfo) -> ErrorChoice {
        self.errors_seen.push(info);
        ErrorChoice::Skip
    }
    fn done(&mut self, outcome: JobOutcome) {
        self.outcome = Some(outcome);
    }
    fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
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
    use std::ffi::OsStr;
    use std::io;
    use std::time::SystemTime;

    use filecommand_core::fs_ops::{ChunkControl, FileSystem, FsMetadata, RawEntry};

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
    /// volume (move-reparse-point-safety design D6).
    pub struct CrossVolume<F: FileSystem>(pub F);

    impl<F: FileSystem> FileSystem for CrossVolume<F> {
        fn metadata(&self, path: &Path) -> io::Result<FsMetadata> {
            self.0.metadata(path)
        }
        fn metadata_follow(&self, path: &Path) -> io::Result<FsMetadata> {
            self.0.metadata_follow(path)
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
        fn copy_stream_chunked(&self, from: &Path, stream: &OsStr, to: &Path, chunk_bytes: usize) -> io::Result<u64> {
            self.0.copy_stream_chunked(from, stream, to, chunk_bytes)
        }
        fn supports_named_streams(&self, path: &Path) -> io::Result<bool> {
            self.0.supports_named_streams(path)
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

    /// The unnamed default stream (`::$DATA`) is not an alternate data
    /// stream. Listing it as one made every real-file copy record a
    /// "stream  not copied" warning, which the junction move test below
    /// would otherwise trip over.
    #[test]
    fn list_streams_on_a_plain_file_names_no_alternate_streams() {
        let temp = TempDir::new("list-streams-plain");
        let file = temp.path().join("plain.txt");
        fs::write(&file, b"plain").unwrap();
        assert_eq!(RealFs::new().list_streams(&file).unwrap(), Vec::<OsString>::new());
        fs::write(temp.path().join("plain.txt:Zone.Identifier"), b"[ZoneTransfer]").unwrap();
        assert_eq!(RealFs::new().list_streams(&file).unwrap(), vec![OsString::from("Zone.Identifier")]);
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
        run_job(&job, &CrossVolume(RealFs::new()), &mut sink);

        assert!(sink.errors_seen.is_empty(), "no error dialog: {:?}", sink.errors_seen);
        assert!(matches!(&sink.outcome, Some(JobOutcome::Completed { skipped }) if skipped.is_empty()), "{:?}", sink.outcome);
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

        assert!(sink.errors_seen.is_empty(), "no error dialog: {:?}", sink.errors_seen);
        assert!(matches!(&sink.outcome, Some(JobOutcome::Completed { skipped }) if skipped.is_empty()), "{:?}", sink.outcome);
        assert!(link.symlink_metadata().is_err(), "the link is gone");
        assert_eq!(fs::read(fx.outside.join("keep.txt")).unwrap(), b"keep me", "the target is intact");
        assert_eq!(fs::read(fx.outside.join("x86").join("a.dll")).unwrap(), b"a.dll");
        assert_eq!(fs::read(fx.src.join("proj").join("file.txt")).unwrap(), b"proj", "the sibling file is untouched");
    }
}
