//! The background `history.json` writer (fuzzy-jump "Directory history
//! persistence"; history-persistence-hardening design D4, D5).
//!
//! `Effect::PersistHistory` used to write the whole file synchronously on
//! the event-loop thread, on every navigation (audit P-08). Now the reducer's
//! snapshot is posted to [`HistoryWriter::submit`], which hands it to one
//! long-lived thread with a single-slot "latest snapshot" mailbox:
//!
//! - older snapshots are dropped unread; only the newest matters;
//! - the thread writes once a burst of changes has settled
//!   ([`DEBOUNCE`] with no new snapshot), and no later than [`MAX_WAIT`]
//!   after the oldest unwritten snapshot arrived;
//! - every write first re-reads the file on disk, migrates it if it is a
//!   legacy file, merges it with the snapshot
//!   ([`config::merge_history_files`]), and replaces the file durably, so
//!   two instances sharing a profile directory keep each other's history;
//! - dropping the writer flushes any pending snapshot and waits for the
//!   thread for a bounded [`FLUSH_WAIT`], so an unreachable profile
//!   directory can never keep the process from exiting.
//!
//! `Runtime` owns the writer, so every exit path from `app::run` (the quit
//! sites, early `?` returns, and panic unwinding) runs the final flush.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use filecommand_core::config::{self, HistoryFile};

use crate::diag;

/// A write happens once this long has passed with no newer snapshot.
pub const DEBOUNCE: Duration = Duration::from_secs(2);
/// ... and no later than this long after the oldest unwritten snapshot.
pub const MAX_WAIT: Duration = Duration::from_secs(10);
/// The most `Drop` waits for the final flush before detaching the thread.
pub const FLUSH_WAIT: Duration = Duration::from_secs(2);

enum Msg {
    Snapshot(HistoryFile),
    Flush,
}

/// Handle to the writer thread. [`HistoryWriter::disabled`] is the no-op
/// used when persistence is off for the session (unwritable profile
/// directory) and in tests that must not touch the disk.
pub struct HistoryWriter {
    tx: Option<Sender<Msg>>,
    done: Option<Receiver<()>>,
    handle: Option<JoinHandle<()>>,
}

impl HistoryWriter {
    /// Start the writer for `path` with the given debounce and maximum
    /// wait. If the thread cannot be started, the failure is logged and a
    /// disabled writer is returned: history is then not persisted this
    /// session, but the session itself continues.
    pub fn spawn(path: PathBuf, debounce: Duration, max_wait: Duration) -> HistoryWriter {
        let (tx, rx) = mpsc::channel::<Msg>();
        let (done_tx, done_rx) = mpsc::channel::<()>();
        let spawned = std::thread::Builder::new().name("history-writer".to_string()).spawn(move || {
            writer_loop(&path, &rx, debounce, max_wait);
            let _ = done_tx.send(());
        });
        match spawned {
            Ok(handle) => HistoryWriter { tx: Some(tx), done: Some(done_rx), handle: Some(handle) },
            Err(e) => {
                diag::log("persist", format_args!("history writer thread could not start: {e}"));
                HistoryWriter::disabled()
            }
        }
    }

    /// A writer that drops every snapshot: persistence is off.
    pub fn disabled() -> HistoryWriter {
        HistoryWriter { tx: None, done: None, handle: None }
    }

    /// Hand the reducer's latest snapshot to the writer thread and return
    /// at once (fuzzy-jump "Navigation does not wait for the history
    /// write"). A no-op when disabled.
    pub fn submit(&self, file: HistoryFile) {
        if let Some(tx) = &self.tx {
            // A send fails only if the thread is gone; nothing to do then.
            let _ = tx.send(Msg::Snapshot(file));
        }
    }

    /// Whether snapshots go anywhere.
    pub fn is_enabled(&self) -> bool {
        self.tx.is_some()
    }
}

impl Drop for HistoryWriter {
    /// Flush the pending snapshot and wait for the thread, for at most
    /// [`FLUSH_WAIT`] (fuzzy-jump "Quitting flushes unwritten history").
    fn drop(&mut self) {
        let Some(tx) = self.tx.take() else { return };
        let _ = tx.send(Msg::Flush);
        drop(tx);
        if let Some(done) = self.done.take() {
            match done.recv_timeout(FLUSH_WAIT) {
                // Finished (or panicked, which also drops its `done_tx`):
                // joining is immediate either way.
                Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                    if let Some(handle) = self.handle.take() {
                        let _ = handle.join();
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    diag::log("persist", format_args!("history writer still busy after {FLUSH_WAIT:?}; exiting without waiting"));
                }
            }
        }
    }
}

/// The writer thread body: keep only the newest snapshot, write it when the
/// burst settles or the maximum wait expires, and on `Flush` (or the handle
/// being dropped) write whatever is pending and exit.
fn writer_loop(path: &Path, rx: &Receiver<Msg>, debounce: Duration, max_wait: Duration) {
    let mut pending: Option<HistoryFile> = None;
    let mut first_pending_at = Instant::now();
    let mut last_submit_at = Instant::now();
    loop {
        let msg = if pending.is_none() {
            rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
        } else {
            let deadline = (last_submit_at + debounce).min(first_pending_at + max_wait);
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        };
        match msg {
            Ok(Msg::Snapshot(file)) => {
                let now = Instant::now();
                if pending.is_none() {
                    first_pending_at = now;
                }
                last_submit_at = now;
                pending = Some(file);
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Some(file) = pending.take() {
                    write_merged(path, &file);
                }
            }
            Ok(Msg::Flush) | Err(RecvTimeoutError::Disconnected) => {
                if let Some(file) = pending.take() {
                    write_merged(path, &file);
                }
                return;
            }
        }
    }
}

/// One write: load the disk copy, migrate it if it is a legacy file, merge
/// it with `mine`, and replace `history.json` durably (design D5, D6). A
/// failure is logged and otherwise swallowed: a history write must never
/// take the session down.
pub fn write_merged(path: &Path, mine: &HistoryFile) {
    let now_epoch = crate::clock::now_unix_ms();
    let mut disk = config::load_history_document(path);
    config::migrate_loaded_history(&mut disk.file, disk.legacy, now_epoch);
    let merged = config::merge_history_files(&disk.file, mine, now_epoch);
    diag::log_err("persist", format_args!("history write {}", path.display()), config::save_history_file_atomic(path, &merged));
}

#[cfg(test)]
mod tests {
    use super::*;
    use filecommand_core::quicksearch::FrecencyEntry;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("filecommand-history-writer-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn snapshot(n: usize) -> HistoryFile {
        HistoryFile {
            commands: (0..=n).map(|i| format!("cmd{i}")).collect(),
            directories: (0..=n).map(|i| FrecencyEntry { path: PathBuf::from(format!(r"C:\d{i}")), visit_count: 1, last_visited_ms: 1_000 + i as u64 }).collect(),
        }
    }

    fn dir_paths(file: &HistoryFile) -> Vec<String> {
        file.directories.iter().map(|d| d.path.to_string_lossy().into_owned()).collect()
    }

    /// Poll for `history.json` to appear, up to `max`.
    fn wait_for_file(path: &Path, max: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < max {
            if path.exists() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        path.exists()
    }

    /// fuzzy-jump "A burst of navigation is written once": 20 rapid
    /// submits settle into one write holding the last snapshot.
    #[test]
    fn a_burst_of_submits_produces_one_write_with_the_last_snapshot() {
        let dir = temp_dir("burst");
        let path = dir.join("history.json");
        let writer = HistoryWriter::spawn(path.clone(), Duration::from_millis(150), Duration::from_secs(5));
        for i in 0..20 {
            writer.submit(snapshot(i));
        }
        assert!(!path.exists(), "nothing is written while the burst is still arriving");
        assert!(wait_for_file(&path, Duration::from_secs(3)), "the write lands once the burst settles");
        let first_write = std::fs::metadata(&path).unwrap().modified().unwrap();
        let on_disk = config::load_history_file(&path);
        assert_eq!(on_disk.commands.len(), 20, "the last snapshot (cmd0..cmd19) was written");
        assert_eq!(dir_paths(&on_disk).len(), 20);
        // With nothing new submitted there is no second write.
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), first_write, "no further write without a new snapshot");
        drop(writer);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// fuzzy-jump "Quitting flushes unwritten history": a submit followed
    /// by drop is written before `drop` returns, debounce notwithstanding.
    #[test]
    fn dropping_the_writer_flushes_the_pending_snapshot_first() {
        let dir = temp_dir("flush");
        let path = dir.join("history.json");
        let writer = HistoryWriter::spawn(path.clone(), Duration::from_secs(60), Duration::from_secs(600));
        writer.submit(HistoryFile { commands: vec!["cd C:\\work".to_string()], directories: vec![FrecencyEntry { path: PathBuf::from(r"C:\work"), visit_count: 1, last_visited_ms: 5 }] });
        drop(writer);
        let on_disk = config::load_history_file(&path);
        assert_eq!(on_disk.commands, vec!["cd C:\\work".to_string()]);
        assert_eq!(dir_paths(&on_disk), vec![r"C:\work"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_disabled_writer_writes_nothing() {
        let dir = temp_dir("disabled");
        let path = dir.join("history.json");
        let writer = HistoryWriter::disabled();
        assert!(!writer.is_enabled());
        writer.submit(snapshot(3));
        drop(writer);
        assert!(!path.exists());
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "no file of any kind was created");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// fuzzy-jump "Two instances keep each other's history" and "Another
    /// instance's commands are kept".
    #[test]
    fn two_writers_on_one_path_keep_both_directories_and_foreign_commands() {
        let dir = temp_dir("two-instances");
        let path = dir.join("history.json");
        // A pre-existing file holding a command neither instance ran.
        config::save_history_file_atomic(&path, &HistoryFile { commands: vec!["cd D:\\build".to_string()], directories: vec![] }).unwrap();

        let a = HistoryWriter::spawn(path.clone(), Duration::from_secs(60), Duration::from_secs(600));
        let b = HistoryWriter::spawn(path.clone(), Duration::from_secs(60), Duration::from_secs(600));
        a.submit(HistoryFile { commands: vec!["cd C:\\a".to_string()], directories: vec![FrecencyEntry { path: PathBuf::from(r"C:\a"), visit_count: 2, last_visited_ms: 10 }] });
        b.submit(HistoryFile { commands: vec!["cd C:\\b".to_string()], directories: vec![FrecencyEntry { path: PathBuf::from(r"C:\b"), visit_count: 1, last_visited_ms: 20 }] });
        drop(a);
        drop(b);

        let on_disk = config::load_history_file(&path);
        let mut paths = dir_paths(&on_disk);
        paths.sort();
        assert_eq!(paths, vec![r"C:\a", r"C:\b"]);
        assert_eq!(on_disk.commands, vec!["cd D:\\build".to_string(), "cd C:\\a".to_string(), "cd C:\\b".to_string()], "the foreign command is kept, ordered before both instances' own");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Writing through the writer adds the format marker and migrates a
    /// legacy disk copy rather than letting its session-relative times win
    /// the merge.
    #[test]
    fn write_merged_migrates_a_legacy_disk_copy_and_stamps_the_version() {
        let dir = temp_dir("legacy-merge");
        let path = dir.join("history.json");
        // A legacy file: no "version", session-relative times.
        std::fs::write(&path, "{\n  \"commands\": [],\n  \"directories\": [\n    {\"path\": \"C:\\\\old\", \"visit_count\": 7, \"last_visited_ms\": 42}\n  ]\n}\n").unwrap();
        let before = crate::clock::now_unix_ms();
        write_merged(&path, &HistoryFile::default());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"version\": 2"), "{text}");
        let loaded = config::load_history_document(&path);
        assert!(!loaded.legacy);
        assert_eq!(loaded.file.directories.len(), 1);
        assert_eq!(loaded.file.directories[0].visit_count, 7, "the count carries over");
        assert!(loaded.file.directories[0].last_visited_ms >= before, "the legacy time was re-stamped to load time");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
