//! Worker-thread mechanism: fulfills `Effect::StartListing` by spawning a
//! background thread that streams directory entries back over a channel as
//! `Command`s, re-entering the same `update` path the main loop uses for
//! key-derived commands. `spawn_job` does the same for `Effect::RunJob`,
//! additionally exposing a cancel flag and a reply channel so the main loop
//! can answer conflict/error dialogs the worker thread blocks on.

use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};

use filecommand_core::fs_ops::{
    CancelFlag, ConflictChoice, ConflictInfo, ErrorChoice, ErrorInfo, Job, JobOutcome, JobSink, ProgressInfo, RealFs,
};
use filecommand_core::listing::{list_dir_chunked, Entry, StdFsReader};
use filecommand_core::panel::parent_path;
use filecommand_core::{Command, PanelSide};

const CHUNK_SIZE: usize = 256;

pub fn spawn_listing(panel: PanelSide, path: PathBuf, tx: Sender<Command>) {
    std::thread::spawn(move || {
        let mut total = 0usize;

        if parent_path(&path).is_some() {
            total += 1;
            if tx.send(Command::ListingChunk { panel, entries: vec![Entry::parent_dir()] }).is_err() {
                return;
            }
        }

        let reader = StdFsReader;
        let result = list_dir_chunked(&reader, &path, CHUNK_SIZE, |chunk| {
            let _ = tx.send(Command::ListingChunk { panel, entries: chunk });
        });

        match result {
            Ok(fs_count) => {
                let _ = tx.send(Command::ListingComplete { panel, total: total + fs_count });
            }
            Err(e) => {
                let _ = tx.send(Command::ListingFailed { panel, message: e.to_string() });
            }
        }
    });
}

/// The main loop's answer to a conflict/error dialog the worker thread is
/// blocked waiting on.
pub enum JobReply {
    Conflict(ConflictChoice),
    Error(ErrorChoice),
}

/// What the main loop keeps for an in-flight job: a flag it can flip to
/// request cancellation, and a channel to forward dialog answers down to
/// the blocked worker thread. Not part of `core::State` — channels/flags
/// are TUI-only plumbing, not pure application state.
pub struct JobHandle {
    pub cancel: CancelFlag,
    pub reply_tx: Sender<JobReply>,
}

struct ChannelSink {
    tx: Sender<Command>,
    reply_rx: mpsc::Receiver<JobReply>,
    cancel: CancelFlag,
    source_dir: PathBuf,
    dest_dir: PathBuf,
}

impl JobSink for ChannelSink {
    fn progress(&mut self, info: ProgressInfo) {
        let _ = self.tx.send(Command::JobProgress(info));
    }

    fn conflict(&mut self, info: ConflictInfo) -> ConflictChoice {
        if self.tx.send(Command::JobConflict(info)).is_err() {
            return ConflictChoice::Skip;
        }
        // Blocks until the main loop answers via `JobHandle::reply_tx` after
        // the user responds to the conflict dialog.
        match self.reply_rx.recv() {
            Ok(JobReply::Conflict(choice)) => choice,
            _ => ConflictChoice::Skip,
        }
    }

    fn error(&mut self, info: ErrorInfo) -> ErrorChoice {
        if self.tx.send(Command::JobError(info)).is_err() {
            return ErrorChoice::Abort;
        }
        match self.reply_rx.recv() {
            Ok(JobReply::Error(choice)) => choice,
            _ => ErrorChoice::Abort,
        }
    }

    fn done(&mut self, outcome: JobOutcome) {
        let _ = self.tx.send(Command::JobDone { outcome, source_dir: self.source_dir.clone(), dest_dir: self.dest_dir.clone() });
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

pub fn spawn_job(job: Job, tx: Sender<Command>) -> JobHandle {
    let cancel = CancelFlag::new();
    let (reply_tx, reply_rx) = mpsc::channel::<JobReply>();
    let handle = JobHandle { cancel: cancel.clone(), reply_tx };
    let source_dir = job.source_dir.clone();
    let dest_dir = job.dest_dir.clone();
    let cancel_for_thread = cancel;
    std::thread::spawn(move || {
        let fs = RealFs::new();
        let mut sink = ChannelSink { tx, reply_rx, cancel: cancel_for_thread, source_dir, dest_dir };
        filecommand_core::fs_ops::run_job(&job, &fs, &mut sink);
    });
    handle
}
