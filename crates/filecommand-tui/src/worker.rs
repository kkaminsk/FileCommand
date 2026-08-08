//! Worker-thread mechanism: fulfills `Effect::StartListing` by spawning a
//! background thread that streams directory entries back over a channel as
//! `Command`s, re-entering the same `update` path the main loop uses for
//! key-derived commands. `spawn_job` does the same for `Effect::RunJob`,
//! additionally exposing a cancel flag and a reply channel so the main loop
//! can answer conflict/error dialogs the worker thread blocks on.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};

use filecommand_core::drives;
use filecommand_core::fs_ops::{
    CancelFlag, ConflictChoice, ConflictInfo, ErrorChoice, ErrorInfo, Job, JobOutcome, JobSink, ProgressInfo, RealFs,
};
use filecommand_core::info::InfoValues;
use filecommand_core::listing::{list_dir_chunked, Entry, FsReader, StdFsReader};
use filecommand_core::panel::parent_path;
use filecommand_core::{Command, PanelSide};

const CHUNK_SIZE: usize = 256;

/// Fetch one drive's volume label off the input path. Absent media and
/// unreachable network shares block here, never in the render loop; the
/// dialog shows the drive with a blank label meanwhile, and a result that
/// arrives after the dialog closed is discarded by `core::update`.
pub fn spawn_drive_label(target: PanelSide, letter: char, tx: Sender<Command>) {
    std::thread::spawn(move || {
        let label = drives::volume_info(letter).map(|(label, _serial)| label);
        let _ = tx.send(Command::DriveLabelResolved { target, letter, label });
    });
}

/// Gather every async Info-panel value for `path` on a worker thread.
///
/// The whole set is sent as one result: these queries hit the same volume
/// and complete together, and a single fill-in keeps the panel from
/// repainting several times in a row.
pub fn spawn_info_query(panel: PanelSide, path: PathBuf, tx: Sender<Command>) {
    std::thread::spawn(move || {
        let values = gather_info(&StdFsReader, &path);
        let _ = tx.send(Command::InfoResolved { panel, path, values });
    });
}

/// The blocking half of an Info query, factored out so it can be driven
/// with an injected reader in tests.
pub fn gather_info(reader: &dyn FsReader, path: &Path) -> InfoValues {
    let drive = drives::drive_letter_of(path);
    let space = drive.and_then(drives::disk_space);
    let volume = drive.and_then(drives::volume_info);
    let (files, dirs) = match reader.read_dir(path) {
        Ok(entries) => {
            let dirs = entries.iter().filter(|e| e.is_dir).count();
            (entries.len() - dirs, dirs)
        }
        // An unreadable directory still resolves — as zero counts rather
        // than a permanent `…` the user cannot clear.
        Err(_) => (0, 0),
    };
    InfoValues {
        memory_bytes: Some(drives::available_memory().unwrap_or(0)),
        drive_total: Some(space.map(|s| s.total).unwrap_or(0)),
        drive_free: Some(space.map(|s| s.free).unwrap_or(0)),
        volume_label: Some(volume.as_ref().map(|(l, _)| l.clone()).unwrap_or_default()),
        serial: Some(volume.map(|(_, s)| s).unwrap_or_else(|| drives::format_serial(0))),
        file_count: Some(files),
        dir_count: Some(dirs),
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use filecommand_core::info::PENDING;
    use filecommand_core::listing::RawDirEntry;
    use std::io;

    struct FakeReader(io::Result<Vec<RawDirEntry>>);

    impl FsReader for FakeReader {
        fn read_dir(&self, _path: &Path) -> io::Result<Vec<RawDirEntry>> {
            match &self.0 {
                Ok(entries) => Ok(entries.clone()),
                Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
            }
        }
    }

    fn raw(name: &str, is_dir: bool) -> RawDirEntry {
        RawDirEntry { name: name.into(), is_dir, size: 0, modified: None }
    }

    #[test]
    fn gather_info_counts_files_and_directories_separately() {
        let reader = FakeReader(Ok(vec![raw("a.txt", false), raw("b.txt", false), raw("sub", true)]));
        let values = gather_info(&reader, Path::new(r"C:\somewhere"));
        assert_eq!(values.file_count, Some(2));
        assert_eq!(values.dir_count, Some(1));
    }

    #[test]
    fn gather_info_always_resolves_every_field() {
        // Even on a path with no drive letter and an unreadable directory,
        // nothing may be left showing `…` forever.
        let reader = FakeReader(Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied")));
        let values = gather_info(&reader, Path::new(r"\\server\share"));
        assert!(values.is_complete(), "an unreadable directory must still resolve, got {values:?}");
        assert_eq!(values.file_count, Some(0));
        assert_eq!(values.dir_count, Some(0));

        let boxes = filecommand_core::info::info_boxes(&values, None);
        let rendered: Vec<&str> = boxes.iter().flat_map(|b| b.fields.iter()).map(|f| f.value.as_str()).collect();
        assert!(!rendered.iter().any(|v| *v == PENDING), "no field is left pending: {rendered:?}");
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
