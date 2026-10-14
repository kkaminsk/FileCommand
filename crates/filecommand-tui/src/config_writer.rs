//! The background `config.toml` writer (application-shell "Slow storage
//! never blocks the event loop"; offload-blocking-io design D9).
//!
//! `Effect::PersistTheme` and `Effect::PersistPanelSplit` used to rewrite
//! `config.toml` synchronously on the event-loop thread — one whole-file
//! read-modify-write per theme apply and per Ctrl+Left/Ctrl+Right step
//! (audit P-09). Now the value is posted to [`ConfigWriter::submit`], which
//! hands it to one long-lived thread with a per-setting mailbox:
//!
//! - writes are applied one at a time, in the order their settings were
//!   first requested, so two rewrites of `config.toml` never interleave
//!   (both saves are read-modify-writes of the same file — running them
//!   concurrently could let one replace overwrite the other's fresh key);
//! - a newer value for a setting replaces an older one the thread has not
//!   taken yet (last write wins), so a held Ctrl+Right produces a couple
//!   of writes rather than one per step, and the final value is what lands;
//! - each write is the same `config::save_theme_atomic` /
//!   `save_panel_split_atomic` call as before — the durable temp-file,
//!   flush and replace sequence (configuration-files "Persisted
//!   configuration writes are durable") is untouched by moving threads —
//!   and a failure is logged, never fatal;
//! - dropping the writer flushes every pending setting and waits for the
//!   thread for a bounded [`FLUSH_WAIT`], so an unreachable profile
//!   directory can never keep the process from exiting.
//!
//! `Runtime` owns the writer, so every exit path from `app::run` (the quit
//! sites, early `?` returns, and panic unwinding) runs the final flush.
//! `history.json` has its own writer (`crate::history_writer`); the two
//! write different files and are independent.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::Duration;

use filecommand_core::config;

use crate::diag;

/// The most `Drop` waits for the final flush before detaching the thread.
pub const FLUSH_WAIT: Duration = Duration::from_secs(2);

/// One configuration setting to persist. Each variant is one key of
/// `config.toml`; the mailbox keeps the newest value per variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigWrite {
    /// `theme = "<name>"` (theme-selection "Applied theme persists to
    /// configuration").
    Theme(String),
    /// `panel_split = <percent>` (panel-split "Split persistence to
    /// configuration").
    PanelSplit(u16),
}

impl ConfigWrite {
    /// Whether `other` is a value for the same setting as `self`.
    fn same_key(&self, other: &ConfigWrite) -> bool {
        matches!((self, other), (ConfigWrite::Theme(_), ConfigWrite::Theme(_)) | (ConfigWrite::PanelSplit(_), ConfigWrite::PanelSplit(_)))
    }
}

/// The save a write is performed with — `config::save_theme_atomic` /
/// `save_panel_split_atomic` in production, injectable for the tests.
type SaveFn = Box<dyn Fn(&Path, &ConfigWrite) -> io::Result<()> + Send>;

enum Msg {
    Write(ConfigWrite),
    Flush,
}

/// Handle to the writer thread. [`ConfigWriter::disabled`] is the no-op
/// used when persistence is off for the session (unwritable profile
/// directory) and in tests that must not touch the disk.
pub struct ConfigWriter {
    tx: Option<Sender<Msg>>,
    done: Option<Receiver<()>>,
    handle: Option<JoinHandle<()>>,
}

impl ConfigWriter {
    /// Start the writer for the `config.toml` at `path`. If the thread
    /// cannot be started, the failure is logged and a disabled writer is
    /// returned: the theme and split are then not persisted this session,
    /// but the session itself continues.
    pub fn spawn(path: PathBuf) -> ConfigWriter {
        ConfigWriter::spawn_with(path, Box::new(save_setting))
    }

    /// [`Self::spawn`] with the per-write save injected (tests).
    pub(crate) fn spawn_with(path: PathBuf, save: SaveFn) -> ConfigWriter {
        let (tx, rx) = mpsc::channel::<Msg>();
        let (done_tx, done_rx) = mpsc::channel::<()>();
        let spawned = std::thread::Builder::new().name("config-writer".to_string()).spawn(move || {
            writer_loop(&path, &rx, &*save);
            let _ = done_tx.send(());
        });
        match spawned {
            Ok(handle) => ConfigWriter { tx: Some(tx), done: Some(done_rx), handle: Some(handle) },
            Err(e) => {
                diag::log("persist", format_args!("config writer thread could not start: {e}"));
                ConfigWriter::disabled()
            }
        }
    }

    /// A writer that drops every write: persistence is off.
    pub fn disabled() -> ConfigWriter {
        ConfigWriter { tx: None, done: None, handle: None }
    }

    /// Hand a setting's new value to the writer thread and return at once.
    /// A no-op when disabled — exactly as the old `persist_enabled` guard
    /// skipped the write.
    pub fn submit(&self, write: ConfigWrite) {
        if let Some(tx) = &self.tx {
            // A send fails only if the thread is gone; nothing to do then.
            let _ = tx.send(Msg::Write(write));
        }
    }

    /// Whether writes go anywhere.
    pub fn is_enabled(&self) -> bool {
        self.tx.is_some()
    }
}

impl Drop for ConfigWriter {
    /// Flush every pending setting and wait for the thread, for at most
    /// [`FLUSH_WAIT`] (application-shell "A setting changed just before
    /// quit is saved", "An unresponsive profile directory cannot hold quit
    /// open").
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
                    diag::log("persist", format_args!("config writer still busy after {FLUSH_WAIT:?}; exiting without waiting"));
                }
            }
        }
    }
}

/// The production save: the same durable write-back the effect bodies used
/// to call inline (config-parsing-robustness owns how it is performed).
fn save_setting(path: &Path, write: &ConfigWrite) -> io::Result<()> {
    match write {
        ConfigWrite::Theme(name) => config::save_theme_atomic(path, name),
        ConfigWrite::PanelSplit(percent) => config::save_panel_split_atomic(path, *percent),
    }
}

/// Store `write` in the mailbox: a value for a setting already queued
/// replaces it in place (last write wins, position kept); a new setting is
/// appended (FIFO by first request).
fn enqueue(pending: &mut Vec<ConfigWrite>, write: ConfigWrite) {
    match pending.iter_mut().find(|p| p.same_key(&write)) {
        Some(slot) => *slot = write,
        None => pending.push(write),
    }
}

/// One write, logged on failure and otherwise swallowed: a configuration
/// write must never take the session down (the setting is already in
/// effect in memory).
fn perform(path: &Path, save: &(dyn Fn(&Path, &ConfigWrite) -> io::Result<()> + Send), write: &ConfigWrite) {
    diag::log_err("persist", format_args!("config write {}", path.display()), save(path, write));
}

/// The writer thread body: drain whatever has arrived into the per-setting
/// mailbox, then write one setting at a time with its newest value; on
/// `Flush` (or the handle being dropped) write every pending setting and
/// exit. Every message queued before the `Flush` is taken into the mailbox
/// first, so nothing submitted before the drop is lost.
fn writer_loop(path: &Path, rx: &Receiver<Msg>, save: &(dyn Fn(&Path, &ConfigWrite) -> io::Result<()> + Send)) {
    let mut pending: Vec<ConfigWrite> = Vec::new();
    loop {
        let msg = if pending.is_empty() { rx.recv().map_err(|_| TryRecvError::Disconnected) } else { rx.try_recv() };
        match msg {
            Ok(Msg::Write(write)) => enqueue(&mut pending, write),
            Err(TryRecvError::Empty) => {
                let next = pending.remove(0);
                perform(path, save, &next);
            }
            Ok(Msg::Flush) | Err(TryRecvError::Disconnected) => {
                for write in pending.drain(..) {
                    perform(path, save, &write);
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("filecommand-config-writer-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The `key = value` line for `key` in the file at `path`, if any.
    fn top_level_value(path: &Path, key: &str) -> Option<String> {
        let text = std::fs::read_to_string(path).ok()?;
        text.lines().find_map(|line| {
            let (k, v) = line.split_once('=')?;
            (k.trim() == key).then(|| v.trim().to_string())
        })
    }

    /// A save hook that performs the real durable write and counts calls.
    fn counting_save(writes: Arc<AtomicUsize>) -> SaveFn {
        Box::new(move |path, write| {
            writes.fetch_add(1, Ordering::SeqCst);
            save_setting(path, write)
        })
    }

    /// application-shell "Rapid split adjustments persist the final value":
    /// eight submissions in a row end with the last value on disk, with no
    /// more writes than submissions.
    #[test]
    fn a_burst_of_split_submissions_ends_with_the_last_value_on_disk() {
        let dir = temp_dir("burst");
        let path = dir.join("config.toml");
        let writes = Arc::new(AtomicUsize::new(0));
        let writer = ConfigWriter::spawn_with(path.clone(), counting_save(writes.clone()));
        for percent in 52..=66u16 {
            if (percent - 50) % 2 == 0 {
                writer.submit(ConfigWrite::PanelSplit(percent));
            }
        }
        drop(writer);
        assert_eq!(top_level_value(&path, "panel_split").as_deref(), Some("66"), "the final value is the last one requested");
        let n = writes.load(Ordering::SeqCst);
        assert!((1..=8).contains(&n), "no more writes than submissions, at least one: {n}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// application-shell "A theme and a split change made together are both
    /// saved": both keys land, and the two saves never overlap.
    #[test]
    fn a_theme_and_a_split_submission_both_land_and_never_run_concurrently() {
        let dir = temp_dir("both");
        let path = dir.join("config.toml");
        let in_save = Arc::new(AtomicBool::new(false));
        let overlap = Arc::new(AtomicBool::new(false));
        let (in_save2, overlap2) = (in_save.clone(), overlap.clone());
        let save: SaveFn = Box::new(move |path, write| {
            if in_save2.swap(true, Ordering::SeqCst) {
                overlap2.store(true, Ordering::SeqCst);
            }
            std::thread::sleep(Duration::from_millis(30));
            let result = save_setting(path, write);
            in_save2.store(false, Ordering::SeqCst);
            result
        });
        let writer = ConfigWriter::spawn_with(path.clone(), save);
        writer.submit(ConfigWrite::Theme("purple-lights".to_string()));
        writer.submit(ConfigWrite::PanelSplit(60));
        drop(writer);
        assert_eq!(top_level_value(&path, "theme").as_deref(), Some("\"purple-lights\""));
        assert_eq!(top_level_value(&path, "panel_split").as_deref(), Some("60"));
        assert!(!overlap.load(Ordering::SeqCst), "the saves ran one at a time");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Settings are written in the order first requested, and a newer value
    /// for a queued setting replaces the older one in place.
    #[test]
    fn the_mailbox_keeps_first_request_order_and_the_newest_value_per_setting() {
        let mut pending = Vec::new();
        enqueue(&mut pending, ConfigWrite::PanelSplit(52));
        enqueue(&mut pending, ConfigWrite::Theme("a".to_string()));
        enqueue(&mut pending, ConfigWrite::PanelSplit(54));
        enqueue(&mut pending, ConfigWrite::Theme("b".to_string()));
        enqueue(&mut pending, ConfigWrite::PanelSplit(56));
        assert_eq!(pending, vec![ConfigWrite::PanelSplit(56), ConfigWrite::Theme("b".to_string())]);
    }

    /// application-shell "A setting changed just before quit is saved":
    /// dropping the writer with a pending write completes it before `drop`
    /// returns.
    #[test]
    fn dropping_the_writer_completes_a_pending_write_first() {
        let dir = temp_dir("flush");
        let path = dir.join("config.toml");
        let save: SaveFn = Box::new(|path, write| {
            std::thread::sleep(Duration::from_millis(200));
            save_setting(path, write)
        });
        let writer = ConfigWriter::spawn_with(path.clone(), save);
        writer.submit(ConfigWrite::PanelSplit(64));
        drop(writer);
        assert_eq!(top_level_value(&path, "panel_split").as_deref(), Some("64"), "the write landed before drop returned");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// application-shell "An unresponsive profile directory cannot hold quit
    /// open": a save that never returns makes `drop` give up after about
    /// [`FLUSH_WAIT`].
    #[test]
    fn a_save_that_blocks_forever_makes_drop_return_after_the_bound() {
        let dir = temp_dir("stuck");
        let path = dir.join("config.toml");
        let save: SaveFn = Box::new(|_, _| loop {
            std::thread::park();
        });
        let writer = ConfigWriter::spawn_with(path, save);
        writer.submit(ConfigWrite::Theme("x".to_string()));
        let started = Instant::now();
        drop(writer);
        let elapsed = started.elapsed();
        assert!(elapsed >= FLUSH_WAIT.mul_f32(0.8), "drop waited for the bound: {elapsed:?}");
        assert!(elapsed < FLUSH_WAIT * 3, "drop did not wait much past the bound: {elapsed:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// With persistence off for the session, `submit` writes nothing.
    #[test]
    fn a_disabled_writer_writes_nothing() {
        let dir = temp_dir("disabled");
        let path = dir.join("config.toml");
        let writer = ConfigWriter::disabled();
        assert!(!writer.is_enabled());
        writer.submit(ConfigWrite::PanelSplit(60));
        writer.submit(ConfigWrite::Theme("x".to_string()));
        drop(writer);
        assert!(!path.exists());
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "no file of any kind was created");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
