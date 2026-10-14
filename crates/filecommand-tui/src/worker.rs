//! Worker-thread mechanism: fulfills `Effect::StartListing` by spawning a
//! background thread that streams directory entries back over a channel as
//! `Command`s, re-entering the same `update` path the main loop uses for
//! key-derived commands. `spawn_job` does the same for `Effect::RunJob`,
//! additionally exposing a cancel flag and a reply channel so the main loop
//! can answer conflict/error dialogs the worker thread blocks on.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use filecommand_core::drives;
use filecommand_core::fs_ops::{
    CancelFlag, ConflictChoice, ConflictInfo, ErrorChoice, ErrorInfo, ItemKind, Job, JobOutcome, JobSink, ProgressInfo, RealFs,
};
use filecommand_core::git_info::{self, GitInfo};
use filecommand_core::info::InfoValues;
use filecommand_core::listing::{find_in_subtree, list_dir_chunked, Entry, FsReader, StdFsReader};
use filecommand_core::panel::parent_path;
use filecommand_core::viewer::search::{find_forward, DEFAULT_CHUNK_SIZE};
use filecommand_core::viewer::ByteSource;
use filecommand_core::{Command, PanelSide};

use crate::diag;

const CHUNK_SIZE: usize = 256;

/// Send a worker result to the event loop, logging a failure rather than
/// dropping it silently (error-diagnostics design D3): the send can only
/// fail once the loop's receiver is gone, i.e. during shutdown. Returns
/// `false` on failure so the caller can stop early — there is no point
/// streaming more into the void.
pub(crate) fn deliver(tx: &Sender<Command>, kind: &str, cmd: Command) -> bool {
    if tx.send(cmd).is_err() {
        diag::log("channel", format_args!("{kind} not delivered: event loop gone"));
        return false;
    }
    true
}

/// The name a status-line error identifies its target by (error-diagnostics
/// design D4): `path.file_name()`, or the whole path when there is none
/// (`C:\`, `\\server\share`).
pub(crate) fn name_or_path(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

/// The `ListingFailed` status-line message, `Cannot read <name>: <err>`
/// (filesystem-error-handling "Error messages name the affected path";
/// design D4): the name rather than the full path, because the panel's
/// mini-status is about half the terminal width and truncates from the
/// right — the panel title directly above already shows the directory.
fn listing_failed_message(path: &Path, err: &io::Error) -> String {
    format!("Cannot read {}: {err}", name_or_path(path))
}

/// How long one git status query may run before its panel's slot reports
/// "no info" and moves on (git-info "Silent absence on timeout and
/// stale-result discarding"; design D3). Generous enough that a normal
/// local-repo status call never hits it, short enough that a stalled
/// network share doesn't leave the branch suffix/marker column in limbo
/// indefinitely.
const GIT_INFO_TIMEOUT: Duration = Duration::from_secs(3);

/// Fetch one drive's volume label off the input path. Absent media and
/// unreachable network shares block here, never in the render loop; the
/// dialog shows the drive with a blank label meanwhile, and a result that
/// arrives after the dialog closed is discarded by `core::update`.
pub fn spawn_drive_label(target: PanelSide, letter: char, generation: u64, tx: Sender<Command>) {
    std::thread::spawn(move || {
        let label = drives::volume_info(letter).map(|(label, _serial)| label);
        deliver(&tx, "drive label", Command::DriveLabelResolved { target, letter, label, generation });
    });
}

/// Gather every async Info-panel value for `path` on a worker thread.
///
/// The whole set is sent as one result: these queries hit the same volume
/// and complete together, and a single fill-in keeps the panel from
/// repainting several times in a row.
pub fn spawn_info_query(panel: PanelSide, path: PathBuf, request: u64, tx: Sender<Command>) {
    std::thread::spawn(move || {
        let values = gather_info(&StdFsReader, &path);
        deliver(&tx, "info query", Command::InfoResolved { panel, path, request, values });
    });
}

/// One panel's git-query slot: at most one status query runs per panel at a
/// time, so rapid navigation cannot pile up concurrent, uncancellable
/// libgit2 walks that compete for I/O (git-info "Rapid navigation does not
/// pile up queries"; git-info-timeout-memory design D3). A dedicated runner
/// per panel — never the shared Info worker — keeps a slow status call on
/// one panel from head-of-line-blocking the other (design D3; git-info
/// "Background repository detection"). The result — resolved
/// branch/statuses, or `GitInfo::none()` for "outside any repository" — is
/// reported back; `core::update`'s generation-key guard
/// (`PanelState::git_request`) is what decides whether a stale reply (one
/// for a directory/generation the panel has since moved past) actually gets
/// applied.
#[derive(Clone, Default)]
pub struct GitQuerySlot {
    state: Arc<Mutex<GitQuerySlotState>>,
}

#[derive(Default)]
struct GitQuerySlotState {
    /// A runner thread is working through the queue.
    running: bool,
    /// The newest request that arrived while the runner was busy; an older
    /// queued request is *replaced*, never queued — its id is already
    /// superseded in `PanelState::git_request`, so its reply would be
    /// dropped anyway.
    pending: Option<(PathBuf, u64)>,
}

impl GitQuerySlot {
    pub fn new() -> GitQuerySlot {
        GitQuerySlot::default()
    }

    /// Route one `Effect::QueryGitInfo` through the slot: start the runner
    /// when the slot is idle, or replace the pending request while a query
    /// is running.
    pub fn request(&self, panel: PanelSide, path: PathBuf, request: u64, tx: Sender<Command>) {
        self.request_with(panel, path, request, tx, GIT_INFO_TIMEOUT, discover_then_query);
    }

    /// `request` with the timeout and the per-directory query injected, so
    /// tests can drive the runner with a scripted fake instead of libgit2.
    /// When a runner is already serving the slot, `query` is unused — the
    /// runner's own query function drives the pending request too
    /// (production always passes the same one).
    fn request_with<F>(&self, panel: PanelSide, path: PathBuf, request: u64, tx: Sender<Command>, timeout: Duration, query: F)
    where
        F: Fn(&Path, &PublishedRoot) -> GitInfo + Send + Sync + 'static,
    {
        let mut state = self.state.lock().unwrap();
        if state.running {
            state.pending = Some((path, request));
            return;
        }
        state.running = true;
        // A pending left over from a panicked runner is superseded by this
        // fresher request, so it is dropped rather than started out of
        // order.
        state.pending = None;
        let slot = self.state.clone();
        std::thread::spawn(move || run_git_query_slot(slot, panel, (path, request), tx, timeout, query));
    }
}

/// Clears the slot's `running` flag if the runner thread unwinds before
/// marking the slot idle itself — otherwise git info for that panel would
/// silently stop for the rest of the session (design D3). The normal exit
/// path disarms the guard after clearing the flag under the lock, so the
/// guard cannot clear it *after* a successor runner has already started.
struct GitQuerySlotGuard {
    state: Arc<Mutex<GitQuerySlotState>>,
    disarmed: bool,
}

impl GitQuerySlotGuard {
    fn disarm(mut self) {
        self.disarmed = true;
    }
}

impl Drop for GitQuerySlotGuard {
    fn drop(&mut self) {
        if self.disarmed {
            return;
        }
        // The lock may be poisoned if the panic happened mid-update.
        self.state.lock().unwrap_or_else(|e| e.into_inner()).running = false;
    }
}

/// The slot's runner: serve the first request, then each pending one in
/// turn, then mark the slot idle. One query at a time per panel; a libgit2
/// walk that outlives its timeout is left running to completion (there is
/// no way to cancel it mid-walk) but its result is dropped — git-info
/// "Silent absence on timeout and stale-result discarding" — and the slot
/// is released when the timeout reply is *sent*, not when the orphaned walk
/// eventually returns, which may be minutes later (design D3).
fn run_git_query_slot<F>(slot: Arc<Mutex<GitQuerySlotState>>, panel: PanelSide, first: (PathBuf, u64), tx: Sender<Command>, timeout: Duration, query: F)
where
    F: Fn(&Path, &PublishedRoot) -> GitInfo + Send + Sync + 'static,
{
    let guard = GitQuerySlotGuard { state: slot.clone(), disarmed: false };
    let query = Arc::new(query);
    let mut current = Some(first);
    while let Some((path, request)) = current {
        let (info, timed_out_root) = run_one_git_query(timeout, &path, &query);
        deliver(&tx, "git info", Command::GitInfoResolved { panel, path, request, info, timed_out_root: timed_out_root.clone() });

        let mut state = slot.lock().unwrap();
        current = state.pending.take();
        // A request queued before this query timed out on root R must not
        // start a second walk of the slow repository — exactly the pile-up
        // this change removes. Its panel shows no git info there, which is
        // what the reducer decides anyway once R is recorded (design D3).
        if let (Some(root), Some((pending_path, _))) = (&timed_out_root, &current) {
            if git_info::is_under_root(pending_path, root) {
                current = None;
            }
        }
        if current.is_none() {
            state.running = false;
        }
    }
    guard.disarm();
}

/// The cell a running query publishes its repository's working-directory
/// root into — *before* the potentially slow status walk starts — so a
/// timeout can report which repository is slow, not just that it is
/// (design D2). `None` while discovery has not finished.
type PublishedRoot = Arc<Mutex<Option<PathBuf>>>;

/// The real per-directory query the runner drives: repository discovery
/// publishes the workdir root (in the queried path's own spelling) before
/// the status walk begins, then branch + status resolve as one blocking
/// libgit2 call sequence (git-info "Background repository detection").
fn discover_then_query(path: &Path, published: &PublishedRoot) -> GitInfo {
    let Some((repo, depth)) = git_info::discover(path) else {
        return GitInfo::none();
    };
    let root = git_info::root_in_dir_spelling(path, depth).unwrap_or_else(|| path.to_path_buf());
    *published.lock().unwrap_or_else(|e| e.into_inner()) = Some(root);
    git_info::query_in_repo(&repo, path)
}

/// Run one query under the timeout on a throwaway inner thread and turn the
/// outcome into the reply's `(info, timed_out_root)`: the resolved info when
/// it finished in time; on timeout `GitInfo::none()` plus the published root
/// (or the queried directory itself when discovery never finished — then
/// only that directory and its descendants are remembered, design D2); on a
/// panicked inner thread `GitInfo::none()` with `timed_out_root: None`, so a
/// one-off panic is not mistaken for a timeout and does not silence a
/// repository for the session.
fn run_one_git_query<F>(timeout: Duration, path: &Path, query: &Arc<F>) -> (GitInfo, Option<PathBuf>)
where
    F: Fn(&Path, &PublishedRoot) -> GitInfo + Send + Sync + 'static,
{
    let published: PublishedRoot = Arc::new(Mutex::new(None));
    let outcome = {
        let published = published.clone();
        let path = path.to_path_buf();
        let query = query.clone();
        race_with_timeout(timeout, move || query(&path, &published))
    };
    match outcome {
        Ok(info) => (info, None),
        Err(RecvTimeoutError::Timeout) => {
            let root = published.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| path.to_path_buf());
            (GitInfo::none(), Some(root))
        }
        Err(RecvTimeoutError::Disconnected) => (GitInfo::none(), None),
    }
}

/// Run `f` on a fresh thread and wait up to `timeout` for its result.
/// `Ok(value)` if `f` finished in time; `Err(RecvTimeoutError::Timeout)`
/// when it did not — `f`'s thread is simply left running to completion in
/// the background (there is no way to cancel an arbitrary blocking call),
/// and its eventual result is dropped rather than delivered late, since
/// nothing is listening for it past the timeout;
/// `Err(RecvTimeoutError::Disconnected)` when `f`'s thread died without
/// sending (a panic), so callers can tell a crash apart from a genuine
/// timeout (git-info-timeout-memory design D2).
fn race_with_timeout<T, F>(timeout: Duration, f: F) -> Result<T, RecvTimeoutError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(timeout)
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

/// Run the F7 streaming search off the render/input thread: a non-matching
/// pattern in a multi-GB file can scan the whole file (bounded per step, but
/// unbounded in total wall time), so this must not block key handling the
/// way `Effect::OpenViewer`'s O(1) open does (viewer: F7 streaming search —
/// "Search is bounded and streaming"). Reopens the file rather than sharing
/// the render loop's cached `ByteSource` across threads — a fresh mmap open
/// is cheap and keeps the two paths independent.
pub fn spawn_viewer_search(path: PathBuf, start_offset: u64, pattern: Vec<u8>, request: u64, tx: Sender<Command>) {
    std::thread::spawn(move || {
        let result = match ByteSource::open(&path) {
            Ok(source) => find_forward(&source, start_offset, &pattern, DEFAULT_CHUNK_SIZE),
            Err(_) => None,
        };
        let (offset, match_range) = match result {
            Some((start, end)) => (Some(start), Some((start, end))),
            None => (None, None),
        };
        deliver(&tx, "viewer search result", Command::ViewerSearchResult { offset, match_range, request });
    });
}

/// Read `path`'s immediate child directories for Tree-mode expansion
/// (`listing::list_child_dirs`) on a worker thread, reporting them back via
/// `Command::TreeNodeExpanded`. A read failure (permission denied, a
/// since-vanished directory) reports an empty child list rather than
/// aborting — `core::update`'s `TreeState::insert_children` still marks the
/// node expanded, so a broken node simply shows no children instead of
/// leaving the query outstanding forever (additional-panel-modes "Children
/// read on expand"; design D7).
pub fn spawn_tree_expand(panel: PanelSide, path: PathBuf, tx: Sender<Command>) {
    std::thread::spawn(move || {
        let children = filecommand_core::listing::list_child_dirs(&StdFsReader, &path).unwrap_or_default();
        deliver(&tx, "tree expansion", Command::TreeNodeExpanded { panel, path, children });
    });
}

/// Run the Alt+F7 subtree name search on a worker thread, streaming each
/// match back via `Command::FindFileMatch` as `listing::find_in_subtree`
/// finds it and finishing with `Command::FindFileSearchDone` — the walk
/// itself performs its own I/O per directory, so results reach the UI
/// incrementally rather than only once the whole subtree has been walked
/// (find-file "Non-blocking search with static progress", "UI stays
/// responsive during a large walk").
pub fn spawn_find_subtree(root: PathBuf, pattern: String, request: u64, tx: Sender<Command>) {
    std::thread::spawn(move || {
        find_in_subtree(&StdFsReader, &root, &pattern, |m| {
            deliver(&tx, "find match", Command::FindFileMatch { request, m });
        });
        deliver(&tx, "find done", Command::FindFileSearchDone { request });
    });
}

pub fn spawn_listing(panel: PanelSide, path: PathBuf, tx: Sender<Command>) {
    std::thread::spawn(move || {
        let mut total = 0usize;

        if parent_path(&path).is_some() {
            total += 1;
            if !deliver(&tx, "listing chunk", Command::ListingChunk { panel, entries: vec![Entry::parent_dir()] }) {
                return;
            }
        }

        let reader = StdFsReader;
        let result = list_dir_chunked(&reader, &path, CHUNK_SIZE, |chunk| {
            deliver(&tx, "listing chunk", Command::ListingChunk { panel, entries: chunk });
        });

        match result {
            Ok(fs_count) => {
                deliver(&tx, "listing complete", Command::ListingComplete { panel, total: total + fs_count });
            }
            Err(e) => {
                // The message names the directory it could not read
                // (filesystem-error-handling "Error messages name the
                // affected path"), not just the bare OS error.
                deliver(&tx, "listing failure", Command::ListingFailed { panel, message: listing_failed_message(&path, &e) });
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
        deliver(&self.tx, "job progress", Command::JobProgress(info));
    }

    fn conflict(&mut self, info: ConflictInfo) -> ConflictChoice {
        if !deliver(&self.tx, "conflict prompt", Command::JobConflict(info)) {
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
        if !deliver(&self.tx, "error prompt", Command::JobError(info)) {
            return ErrorChoice::Abort;
        }
        match self.reply_rx.recv() {
            Ok(JobReply::Error(choice)) => choice,
            _ => ErrorChoice::Abort,
        }
    }

    fn done(&mut self, outcome: JobOutcome) {
        // Metadata-restore warnings the engine recorded are logged under
        // `copy`, so a partial failure leaves a diagnostic trail even when
        // the user dismisses the end-of-job summary unread
        // (error-diagnostics design D3; operation-dialogs "Metadata-restore
        // failures are reported in the end-of-job summary").
        let items = match &outcome {
            JobOutcome::Completed { skipped } | JobOutcome::Cancelled { skipped } => skipped,
        };
        for item in items {
            if item.kind == ItemKind::Warning {
                diag::log("copy", format_args!("{}: {}", item.path.display(), item.reason));
            }
        }
        deliver(&self.tx, "job outcome", Command::JobDone { outcome, source_dir: self.source_dir.clone(), dest_dir: self.dest_dir.clone() });
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
    use std::io::Write;

    fn temp_file(name: &str, contents: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("filecommand-tui-worker-viewer-test-{}-{}", std::process::id(), name));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("file.bin");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents).unwrap();
        f.flush().unwrap();
        path
    }

    #[test]
    fn spawn_viewer_search_reports_the_match_back_on_the_channel() {
        let path = temp_file("found", b"the quick brown fox jumps over the lazy dog");
        let (tx, rx) = mpsc::channel();
        spawn_viewer_search(path, 0, b"brown".to_vec(), 7, tx);
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Command::ViewerSearchResult { offset, match_range, request }) => {
                assert_eq!(offset, Some(10));
                assert_eq!(match_range, Some((10, 15)));
                assert_eq!(request, 7, "the request id must be echoed back unchanged");
            }
            other => panic!("expected ViewerSearchResult, got {other:?}"),
        }
    }

    #[test]
    fn spawn_viewer_search_reports_no_match_as_none() {
        let path = temp_file("not-found", b"the quick brown fox");
        let (tx, rx) = mpsc::channel();
        spawn_viewer_search(path, 0, b"zebra".to_vec(), 1, tx);
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Command::ViewerSearchResult { offset: None, match_range: None, request: 1 }) => {}
            other => panic!("expected a no-match ViewerSearchResult, got {other:?}"),
        }
    }

    #[test]
    fn spawn_viewer_search_on_an_unopenable_path_reports_no_match_instead_of_panicking() {
        let missing = std::env::temp_dir().join("filecommand-tui-worker-viewer-test-does-not-exist.bin");
        let (tx, rx) = mpsc::channel();
        spawn_viewer_search(missing, 0, b"x".to_vec(), 1, tx);
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Command::ViewerSearchResult { offset: None, match_range: None, request: 1 }) => {}
            other => panic!("expected a no-match ViewerSearchResult, got {other:?}"),
        }
    }

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
        RawDirEntry { name: name.into(), is_dir, size: 0, modified: None, hidden: false }
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

    #[test]
    fn race_with_timeout_returns_the_value_when_it_finishes_in_time() {
        let result = race_with_timeout(std::time::Duration::from_secs(5), || 42);
        assert_eq!(result, Ok(42));
    }

    #[test]
    fn race_with_timeout_gives_up_and_reports_timeout_on_a_slow_call() {
        let result = race_with_timeout(std::time::Duration::from_millis(20), || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            "too slow"
        });
        assert!(matches!(result, Err(RecvTimeoutError::Timeout)));
    }

    #[test]
    fn race_with_timeout_reports_a_panicked_call_as_disconnected_not_timeout() {
        // design D2: a crash must be distinguishable from a genuine timeout.
        let result = race_with_timeout(std::time::Duration::from_secs(5), || panic!("boom"));
        assert!(matches!(result, Err(RecvTimeoutError::Disconnected)));
    }

    // -----------------------------------------------------------------
    // git-info-timeout-memory: one query in flight per panel, and timeout
    // memory (design D2/D3)
    // -----------------------------------------------------------------

    /// A scripted stand-in for the real per-directory git query: records the
    /// order queries start in and runs a per-path script, so slot-runner
    /// tests can block, publish roots, and panic on cue without libgit2.
    struct FakeGitQuery {
        started: Sender<PathBuf>,
        scripts: Mutex<std::collections::HashMap<PathBuf, FakeScript>>,
    }

    enum FakeScript {
        /// Return this info immediately.
        Ready(GitInfo),
        /// Publish `root` as the discovered repository root, then block
        /// until the gate fires (a slow status walk).
        PublishThenBlock(PathBuf, mpsc::Receiver<()>),
        /// Block on the gate without publishing anything (discovery itself
        /// is the slow part).
        Block(mpsc::Receiver<()>),
        /// Die without answering (a panic inside the query).
        Panic,
    }

    impl FakeGitQuery {
        fn new(started: Sender<PathBuf>, scripts: Vec<(PathBuf, FakeScript)>) -> Arc<FakeGitQuery> {
            Arc::new(FakeGitQuery { started, scripts: Mutex::new(scripts.into_iter().collect()) })
        }

        fn run(&self, path: &Path, published: &PublishedRoot) -> GitInfo {
            let _ = self.started.send(path.to_path_buf());
            // Bound ahead of the match so the lock guard's temporary is
            // dropped before any script runs — a `Panic` script must not
            // poison the map for the next runner.
            let script = self.scripts.lock().unwrap().remove(path);
            match script {
                Some(FakeScript::Ready(info)) => info,
                Some(FakeScript::PublishThenBlock(root, gate)) => {
                    *published.lock().unwrap() = Some(root);
                    let _ = gate.recv();
                    GitInfo { branch: Some("late".to_string()), ..Default::default() }
                }
                Some(FakeScript::Block(gate)) => {
                    let _ = gate.recv();
                    GitInfo { branch: Some("late".to_string()), ..Default::default() }
                }
                Some(FakeScript::Panic) => panic!("scripted query panic"),
                None => GitInfo::none(),
            }
        }
    }

    fn fake_query(fake: &Arc<FakeGitQuery>) -> impl Fn(&Path, &PublishedRoot) -> GitInfo + Send + Sync + 'static {
        let fake = fake.clone();
        move |path, published| fake.run(path, published)
    }

    /// Receive the next `GitInfoResolved` reply, panicking on anything else.
    fn recv_git_reply(rx: &mpsc::Receiver<Command>) -> (PathBuf, u64, GitInfo, Option<PathBuf>) {
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Command::GitInfoResolved { panel: _, path, request, info, timed_out_root }) => (path, request, info, timed_out_root),
            other => panic!("expected GitInfoResolved, got {other:?}"),
        }
    }

    #[test]
    fn a_superseded_pending_request_is_never_started() {
        // git-info "Rapid navigation does not pile up queries": with the
        // first query still running, two more navigations queue; when the
        // first reports, only the *last* queued directory is queried.
        let (started_tx, started_rx) = mpsc::channel::<PathBuf>();
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let fake = FakeGitQuery::new(
            started_tx,
            vec![
                (PathBuf::from("/first"), FakeScript::Block(gate_rx)),
                (PathBuf::from("/third"), FakeScript::Ready(GitInfo { branch: Some("third-branch".to_string()), ..Default::default() })),
            ],
        );
        let slot = GitQuerySlot::new();
        let (tx, rx) = mpsc::channel();
        let timeout = std::time::Duration::from_secs(5);

        slot.request_with(PanelSide::Left, PathBuf::from("/first"), 1, tx.clone(), timeout, fake_query(&fake));
        assert_eq!(started_rx.recv_timeout(std::time::Duration::from_secs(5)), Ok(PathBuf::from("/first")), "the first query starts");
        slot.request_with(PanelSide::Left, PathBuf::from("/second"), 2, tx.clone(), timeout, fake_query(&fake));
        slot.request_with(PanelSide::Left, PathBuf::from("/third"), 3, tx.clone(), timeout, fake_query(&fake));
        gate_tx.send(()).unwrap(); // let the first query finish within its timeout

        let (path, request, _info, timed_out_root) = recv_git_reply(&rx);
        assert_eq!((path, request, timed_out_root), (PathBuf::from("/first"), 1, None), "the first query reports normally");
        assert_eq!(started_rx.recv_timeout(std::time::Duration::from_secs(5)), Ok(PathBuf::from("/third")), "the newest queued request runs next");
        let (path, request, info, timed_out_root) = recv_git_reply(&rx);
        assert_eq!((path, request, timed_out_root), (PathBuf::from("/third"), 3, None));
        assert_eq!(info.branch.as_deref(), Some("third-branch"));
        assert!(
            matches!(started_rx.recv_timeout(std::time::Duration::from_millis(300)), Err(RecvTimeoutError::Timeout)),
            "the superseded second request is never started"
        );
        assert!(rx.recv_timeout(std::time::Duration::from_millis(300)).is_err(), "and never answered");
    }

    #[test]
    fn a_pending_request_inside_the_timed_out_repository_is_never_started() {
        // git-info "Waiting query inside the timed-out repository is not
        // started": the running query publishes root R and then times out;
        // the request queued under R meanwhile is discarded rather than
        // starting a second walk of the slow repository (design D3).
        let (started_tx, started_rx) = mpsc::channel::<PathBuf>();
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let fake =
            FakeGitQuery::new(started_tx, vec![(PathBuf::from("/repo/src"), FakeScript::PublishThenBlock(PathBuf::from("/repo"), gate_rx))]);
        let slot = GitQuerySlot::new();
        let (tx, rx) = mpsc::channel();
        let timeout = std::time::Duration::from_millis(300);

        slot.request_with(PanelSide::Left, PathBuf::from("/repo/src"), 1, tx.clone(), timeout, fake_query(&fake));
        assert_eq!(started_rx.recv_timeout(std::time::Duration::from_secs(5)), Ok(PathBuf::from("/repo/src")));
        // Queued while the first query is still walking (before it times
        // out), inside the about-to-be-remembered repository.
        slot.request_with(PanelSide::Left, PathBuf::from("/repo/docs"), 2, tx, timeout, fake_query(&fake));

        let (_path, request, info, timed_out_root) = recv_git_reply(&rx);
        assert_eq!(request, 1);
        assert_eq!(info, GitInfo::none(), "a timeout degrades to no info");
        assert_eq!(timed_out_root, Some(PathBuf::from("/repo")), "the published root is reported");
        assert!(
            matches!(started_rx.recv_timeout(std::time::Duration::from_millis(500)), Err(RecvTimeoutError::Timeout)),
            "the queued request inside the timed-out repository never starts"
        );
        assert!(rx.recv_timeout(std::time::Duration::from_millis(300)).is_err(), "and it is never answered");
        drop(gate_tx); // let the orphaned fake walk finish
    }

    #[test]
    fn a_timeout_reports_the_published_repository_root() {
        // design D2: discovery finished — the root was published — before
        // the status walk stalled.
        let (started_tx, _started_rx) = mpsc::channel::<PathBuf>();
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let fake =
            FakeGitQuery::new(started_tx, vec![(PathBuf::from("/repo/src"), FakeScript::PublishThenBlock(PathBuf::from("/repo"), gate_rx))]);
        let slot = GitQuerySlot::new();
        let (tx, rx) = mpsc::channel();
        slot.request_with(PanelSide::Left, PathBuf::from("/repo/src"), 1, tx, std::time::Duration::from_millis(100), fake_query(&fake));

        let (path, request, info, timed_out_root) = recv_git_reply(&rx);
        assert_eq!(path, PathBuf::from("/repo/src"));
        assert_eq!(request, 1);
        assert_eq!(info, GitInfo::none());
        assert_eq!(timed_out_root, Some(PathBuf::from("/repo")));
        drop(gate_tx);
    }

    #[test]
    fn a_timeout_before_discovery_finishes_reports_the_queried_directory() {
        // git-info "Repository not located before the timeout": with no
        // root published, the queried directory itself is reported (only it
        // and its descendants are remembered).
        let (started_tx, _started_rx) = mpsc::channel::<PathBuf>();
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let fake = FakeGitQuery::new(started_tx, vec![(PathBuf::from("/slow/deep"), FakeScript::Block(gate_rx))]);
        let slot = GitQuerySlot::new();
        let (tx, rx) = mpsc::channel();
        slot.request_with(PanelSide::Left, PathBuf::from("/slow/deep"), 1, tx, std::time::Duration::from_millis(100), fake_query(&fake));

        let (path, request, info, timed_out_root) = recv_git_reply(&rx);
        assert_eq!(path, PathBuf::from("/slow/deep"));
        assert_eq!(request, 1);
        assert_eq!(info, GitInfo::none());
        assert_eq!(timed_out_root, Some(PathBuf::from("/slow/deep")), "the queried directory stands in for the unlocated root");
        drop(gate_tx);
    }

    #[test]
    fn a_panicked_query_reports_no_timed_out_root_and_leaves_the_slot_idle() {
        // design D2/D3: a crash is not a timeout (`timed_out_root: None`),
        // and the slot is usable for the next request afterwards.
        let (started_tx, started_rx) = mpsc::channel::<PathBuf>();
        let fake = FakeGitQuery::new(
            started_tx,
            vec![
                (PathBuf::from("/panic"), FakeScript::Panic),
                (PathBuf::from("/ok"), FakeScript::Ready(GitInfo { branch: Some("ok-branch".to_string()), ..Default::default() })),
            ],
        );
        let slot = GitQuerySlot::new();
        let (tx, rx) = mpsc::channel();
        let timeout = std::time::Duration::from_secs(5);

        slot.request_with(PanelSide::Left, PathBuf::from("/panic"), 1, tx.clone(), timeout, fake_query(&fake));
        assert_eq!(started_rx.recv_timeout(std::time::Duration::from_secs(5)), Ok(PathBuf::from("/panic")), "the first query starts");
        let (_path, request, info, timed_out_root) = recv_git_reply(&rx);
        assert_eq!(request, 1);
        assert_eq!(info, GitInfo::none());
        assert_eq!(timed_out_root, None, "a panic is not a timeout");

        // With nothing queued, the runner marks the slot idle and exits.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while slot.state.lock().unwrap().running {
            assert!(std::time::Instant::now() < deadline, "the slot is marked idle after the panicked query");
            std::thread::yield_now();
        }

        slot.request_with(PanelSide::Left, PathBuf::from("/ok"), 2, tx, timeout, fake_query(&fake));
        assert_eq!(started_rx.recv_timeout(std::time::Duration::from_secs(5)), Ok(PathBuf::from("/ok")), "an idle slot serves the next request");
        let (_path, request, info, timed_out_root) = recv_git_reply(&rx);
        assert_eq!(request, 2);
        assert_eq!(info.branch.as_deref(), Some("ok-branch"));
        assert_eq!(timed_out_root, None);
    }

    // -------------------------------------------------------------
    // error-diagnostics: the listing failure message names the directory
    // (filesystem-error-handling "Error messages name the affected path";
    // task 4.3)
    // -------------------------------------------------------------

    #[test]
    fn listing_failed_message_names_the_directory() {
        // Scenario "Access-denied listing names the directory": `Cannot
        // read <directory>: <OS message>`.
        let err = io::Error::new(io::ErrorKind::PermissionDenied, "Access is denied. (os error 5)");
        let dir = Path::new("Windows").join("CSC");
        assert_eq!(listing_failed_message(&dir, &err), "Cannot read CSC: Access is denied. (os error 5)");
    }

    #[test]
    fn listing_failed_message_keeps_the_whole_path_for_a_root() {
        // A drive root (`C:\`) or share root (`\\server\share`) has no file
        // name, so the whole path stands in (design D4).
        let root = if cfg!(windows) { r"C:\" } else { "/" };
        let err = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        assert_eq!(listing_failed_message(Path::new(root), &err), format!("Cannot read {root}: denied"));
    }

    #[test]
    fn spawn_listing_of_a_missing_directory_reports_cannot_read_naming_it() {
        // End to end through the worker thread: the `ListingFailed` command
        // carries the prefixed message, not a bare OS error.
        let dir = std::env::temp_dir().join(format!("filecommand-tui-worker-listing-fail-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let (tx, rx) = mpsc::channel();
        spawn_listing(PanelSide::Left, dir, tx);
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(5)) {
                Ok(Command::ListingChunk { .. }) => continue, // the synthetic `..` entry
                Ok(Command::ListingFailed { panel, message }) => {
                    assert_eq!(panel, PanelSide::Left);
                    assert!(message.starts_with(&format!("Cannot read {name}: ")), "names the directory and the OS reason: {message}");
                    break;
                }
                other => panic!("expected ListingChunk or ListingFailed, got {other:?}"),
            }
        }
    }

    #[test]
    fn spawn_find_subtree_streams_matches_then_reports_done() {
        let dir = std::env::temp_dir().join(format!("filecommand-tui-worker-find-file-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("report.txt"), b"x").unwrap();
        std::fs::write(dir.join("sub").join("report2.txt"), b"x").unwrap();
        std::fs::write(dir.join("other.txt"), b"x").unwrap();

        let (tx, rx) = mpsc::channel();
        spawn_find_subtree(dir.clone(), "report".to_string(), 7, tx);

        let mut matched_names = Vec::new();
        let saw_done;
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(5)) {
                Ok(Command::FindFileMatch { request, m }) => {
                    assert_eq!(request, 7);
                    matched_names.push(m.entry.name.to_string_lossy().into_owned());
                }
                Ok(Command::FindFileSearchDone { request }) => {
                    assert_eq!(request, 7);
                    saw_done = true;
                    break;
                }
                other => panic!("unexpected message: {other:?}"),
            }
        }
        matched_names.sort();
        assert_eq!(matched_names, vec!["report.txt".to_string(), "report2.txt".to_string()]);
        assert!(saw_done);
        let _ = std::fs::remove_dir_all(&dir);
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
