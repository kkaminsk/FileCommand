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
/// hanging.
#[derive(Default)]
struct RecordingSink {
    conflicts_seen: usize,
    outcome: Option<JobOutcome>,
    cancel: CancelFlag,
}

impl JobSink for RecordingSink {
    fn progress(&mut self, _info: ProgressInfo) {}
    fn conflict(&mut self, _info: ConflictInfo) -> ConflictChoice {
        self.conflicts_seen += 1;
        ConflictChoice::Overwrite
    }
    fn error(&mut self, _info: ErrorInfo) -> ErrorChoice {
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
