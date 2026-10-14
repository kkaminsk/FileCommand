//! Worker-thread mechanism: fulfills `Effect::StartListing` by spawning a
//! background thread that streams directory entries back over a channel as
//! `Command`s, re-entering the same `update` path the main loop uses for
//! key-derived commands. `spawn_job` does the same for `Effect::RunJob`,
//! additionally exposing a cancel flag and a reply channel so the main loop
//! can answer conflict/error dialogs the worker thread blocks on.

use std::any::Any;
use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use filecommand_core::drives;
use filecommand_core::editor::{EditorState, LoadResult};
use filecommand_core::fs_ops::path::to_extended_path;
use filecommand_core::fs_ops::{
    CancelFlag, ConflictChoice, ConflictInfo, ErrorChoice, ErrorInfo, ItemKind, Job, JobOutcome, JobSink, ProgressInfo, ProgressThrottle, RealFs,
    SkippedItem,
};
use filecommand_core::git_info::{self, GitInfo};
use filecommand_core::info::InfoValues;
use filecommand_core::listing::{find_in_subtree, list_dir_streaming, BatchPolicy, Entry, FsReader, StdFsReader};
use filecommand_core::panel::parent_path;
use filecommand_core::viewer::search::{find_forward, DEFAULT_CHUNK_SIZE};
use filecommand_core::viewer::ByteSource;
use filecommand_core::{Command, PanelSide, ProbeKind};

use crate::diag;

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

/// Run `body` under `catch_unwind`, turning a panic into its message
/// (process-lifecycle-hardening design D6). Every worker body runs through
/// this so a panic becomes that worker's normal failure reply instead of a
/// `Reading… N` or a progress dialog that never closes: the panic hook
/// itself does nothing on a non-UI thread (design D4), so this is the only
/// place the payload is seen.
pub(crate) fn guarded<T>(body: impl FnOnce() -> T) -> Result<T, String> {
    panic::catch_unwind(AssertUnwindSafe(body)).map_err(|payload| panic_message(payload.as_ref()))
}

/// The text of a panic payload: the `&str` or `String` `panic!` was given,
/// or `"unknown panic"` for anything else.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Record a contained worker panic in the diagnostic log (error-diagnostics
/// coordination): with the hook silent off the UI thread, the log and the
/// failure reply are the only places the message appears.
fn log_worker_panic(kind: &str, message: &str) {
    diag::log("worker", format_args!("{kind} worker panicked: {message}"));
}

/// Spawn one worker thread that runs `body` under [`guarded`] and hands the
/// outcome — the body's value, or the panic message — to `reply`, which
/// sends the worker's result (or its defined failure reply, design D6) to
/// the event loop. The gauge token is held for the thread's whole lifetime,
/// so it is released after the reply's last send even when the body
/// panicked (idle-aware-event-loop design D3).
fn spawn_worker<T, B, R>(token: WorkerToken, body: B, reply: R)
where
    T: Send + 'static,
    B: FnOnce() -> T + Send + 'static,
    R: FnOnce(Result<T, String>) + Send + 'static,
{
    std::thread::spawn(move || {
        let _token = token;
        reply(guarded(body));
    });
}

/// A live count of the worker threads that can still post to the event
/// loop's channel (idle-aware-event-loop design D3): while it is nonzero the
/// loop polls for terminal input at the usual ~33 ms cadence so results keep
/// today's latency; at zero the loop sleeps until the next time-driven
/// deadline instead. Every `spawn_*` moves a [`WorkerToken`] into its thread.
#[derive(Clone, Default)]
pub struct WorkerGauge {
    live: Arc<AtomicUsize>,
}

impl WorkerGauge {
    pub fn new() -> WorkerGauge {
        WorkerGauge::default()
    }

    /// The number of worker threads currently holding a token.
    pub fn live(&self) -> usize {
        self.live.load(Ordering::Acquire)
    }

    /// A token keeping one worker counted until it is dropped. Created by the
    /// caller *before* the thread starts — an increment from inside the
    /// thread would leave a window where the loop sees zero live workers and
    /// parks while the worker is about to post.
    pub fn token(&self) -> WorkerToken {
        self.live.fetch_add(1, Ordering::AcqRel);
        WorkerToken { live: self.live.clone() }
    }
}

/// The RAII half of [`WorkerGauge`]: moved into the worker thread, so a
/// normal return *or* a panic decrements the count — always after the
/// thread's last channel send, with release ordering so a loop that reads
/// zero (acquire) then drains the channel once more is guaranteed to see
/// that send (idle-aware-event-loop design D3).
pub struct WorkerToken {
    live: Arc<AtomicUsize>,
}

impl Drop for WorkerToken {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::Release);
    }
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
pub fn spawn_drive_label(target: PanelSide, letter: char, generation: u64, tx: Sender<Command>, token: WorkerToken) {
    spawn_drive_label_with(target, letter, generation, tx, token, move || drives::volume_info(letter).map(|(label, _serial)| label));
}

/// `spawn_drive_label` with the lookup injected (a test seam for the
/// panic reply, design D6): a panicked lookup resolves to no label, exactly
/// like absent media.
fn spawn_drive_label_with<B>(target: PanelSide, letter: char, generation: u64, tx: Sender<Command>, token: WorkerToken, body: B)
where
    B: FnOnce() -> Option<String> + Send + 'static,
{
    spawn_worker(token, body, move |result| {
        let label = result.unwrap_or_else(|msg| {
            log_worker_panic("drive label", &msg);
            None
        });
        deliver(&tx, "drive label", Command::DriveLabelResolved { target, letter, label, generation });
    });
}

/// Gather every async Info-panel value for `path` on a worker thread.
///
/// The whole set is sent as one result: these queries hit the same volume
/// and complete together, and a single fill-in keeps the panel from
/// repainting several times in a row.
pub fn spawn_info_query(panel: PanelSide, path: PathBuf, request: u64, tx: Sender<Command>, token: WorkerToken) {
    spawn_info_query_with(panel, path, request, tx, token, |path| gather_info(&StdFsReader, path));
}

/// `spawn_info_query` with the gathering injected (design D6): a panic
/// resolves every value to empty or zero, as for an unreadable directory,
/// so no field is left showing `…` forever.
fn spawn_info_query_with<B>(panel: PanelSide, path: PathBuf, request: u64, tx: Sender<Command>, token: WorkerToken, body: B)
where
    B: FnOnce(&Path) -> InfoValues + Send + 'static,
{
    let body_path = path.clone();
    spawn_worker(
        token,
        move || body(&body_path),
        move |result| {
            let values = result.unwrap_or_else(|msg| {
                log_worker_panic("info query", &msg);
                unreadable_info()
            });
            deliver(&tx, "info query", Command::InfoResolved { panel, path, request, values });
        },
    );
}

/// Every Info value resolved to empty or zero — what `gather_info` reports
/// for a directory it cannot read and no drive letter, and the panic reply
/// of the Info worker (design D6).
fn unreadable_info() -> InfoValues {
    InfoValues {
        memory_bytes: Some(0),
        drive_total: Some(0),
        drive_free: Some(0),
        volume_label: Some(String::new()),
        serial: Some(drives::format_serial(0)),
        file_count: Some(0),
        dir_count: Some(0),
    }
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
    pub fn request(&self, panel: PanelSide, path: PathBuf, request: u64, tx: Sender<Command>, token: WorkerToken) {
        self.request_with(panel, path, request, tx, token, GIT_INFO_TIMEOUT, discover_then_query);
    }

    /// `request` with the timeout and the per-directory query injected, so
    /// tests can drive the runner with a scripted fake instead of libgit2.
    /// When a runner is already serving the slot, `query` is unused — the
    /// runner's own query function drives the pending request too
    /// (production always passes the same one).
    #[allow(clippy::too_many_arguments)]
    fn request_with<F>(&self, panel: PanelSide, path: PathBuf, request: u64, tx: Sender<Command>, token: WorkerToken, timeout: Duration, query: F)
    where
        F: Fn(&Path, &PublishedRoot) -> GitInfo + Send + Sync + 'static,
    {
        let mut state = self.state.lock().unwrap();
        if state.running {
            // The token stays with the already-running runner, which keeps
            // the gauge honest for as long as queued requests can still be
            // served; this request's own token is dropped (the decrement
            // balances its `token()` call).
            drop(token);
            state.pending = Some((path, request));
            return;
        }
        state.running = true;
        // A pending left over from a panicked runner is superseded by this
        // fresher request, so it is dropped rather than started out of
        // order.
        state.pending = None;
        let slot = self.state.clone();
        std::thread::spawn(move || run_git_query_slot(slot, panel, (path, request), tx, token, timeout, query));
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
fn run_git_query_slot<F>(
    slot: Arc<Mutex<GitQuerySlotState>>,
    panel: PanelSide,
    first: (PathBuf, u64),
    tx: Sender<Command>,
    token: WorkerToken,
    timeout: Duration,
    query: F,
) where
    F: Fn(&Path, &PublishedRoot) -> GitInfo + Send + Sync + 'static,
{
    // Held for the runner's whole lifetime — including a panic unwind — so
    // the gauge counts this slot until its last reply has been sent
    // (idle-aware-event-loop design D3).
    let _token = token;
    let guard = GitQuerySlotGuard { state: slot.clone(), disarmed: false };
    let query = Arc::new(query);
    let mut current = Some(first);
    while let Some((path, request)) = current {
        // The query itself already runs on `race_with_timeout`'s inner
        // thread, whose panic surfaces as `Disconnected`; this guards the
        // runner's own half so a panic there still answers "no info"
        // (process-lifecycle-hardening design D6).
        let (info, timed_out_root) = guarded(|| run_one_git_query(timeout, &path, &query)).unwrap_or_else(|msg| {
            log_worker_panic("git info", &msg);
            (GitInfo::none(), None)
        });
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
        // Directory links count as directories, like the panel lists them;
        // entries skipped as unreadable are not counted (link-aware-listing
        // design D6).
        Ok(listing) => {
            let dirs = listing.entries.iter().filter(|e| e.is_dir).count();
            (listing.entries.len() - dirs, dirs)
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
/// the render loop's cached `ByteSource` across threads — a second
/// positioned-read open is cheap and keeps the two paths independent. The
/// source is a local, so the worker's handle is released when the search
/// returns (viewer: Instant open — "Closing the viewer releases the file").
///
/// `cancel` is the search's stop signal (viewer "Search stops when
/// superseded or when the viewer closes"; listing-request-guard design D5):
/// once set, `find_forward` returns at its next chunk boundary and no
/// `ViewerSearchResult` is sent — the reducer has already superseded this
/// id, so a reply would be dropped anyway.
pub fn spawn_viewer_search(
    path: PathBuf,
    start_offset: u64,
    pattern: Vec<u8>,
    request: u64,
    cancel: CancelFlag,
    tx: Sender<Command>,
    token: WorkerToken,
) {
    let scan_cancel = cancel.clone();
    spawn_viewer_search_with(request, cancel, tx, token, move || match ByteSource::open(&path) {
        Ok(source) => find_forward(&source, start_offset, &pattern, DEFAULT_CHUNK_SIZE, &scan_cancel),
        Err(_) => None,
    });
}

/// `spawn_viewer_search` with the scan injected (process-lifecycle-hardening
/// design D6): a panicked scan reports "no match", the same reply as an
/// unopenable file. A cancelled search sends nothing at all.
fn spawn_viewer_search_with<B>(request: u64, cancel: CancelFlag, tx: Sender<Command>, token: WorkerToken, body: B)
where
    B: FnOnce() -> Option<(u64, u64)> + Send + 'static,
{
    spawn_worker(token, body, move |result| {
        let found = result.unwrap_or_else(|msg| {
            log_worker_panic("viewer search", &msg);
            None
        });
        if cancel.is_cancelled() {
            return;
        }
        let (offset, match_range) = match found {
            Some((start, end)) => (Some(start), Some((start, end))),
            None => (None, None),
        };
        deliver(&tx, "viewer search result", Command::ViewerSearchResult { offset, match_range, request });
    });
}

// ---------------------------------------------------------------------
// offload-blocking-io: the command-line target probe, the F3/F4 opens, the
// editor save and the associated-application launch all run here rather
// than on the event-loop thread (application-shell "The reducer performs no
// filesystem I/O", "Slow storage never blocks the event loop"). Each worker
// runs under `spawn_worker`'s `catch_unwind` and turns a panic into its own
// failure reply carrying the request id (design D8).
// ---------------------------------------------------------------------

/// The status-line error forms of filesystem-error-handling "Error messages
/// name the affected path" (error-diagnostics design D4): `Cannot
/// <verb> <name>: <reason>`, where the name is the file's own name — the
/// panel title directly above already shows the directory, and a full path
/// would push the reason off the truncated mini-status — or the whole path
/// when there is no file name (a drive or share root).
pub(crate) fn cannot_message(verb: &str, path: &Path, reason: impl std::fmt::Display) -> String {
    format!("Cannot {verb} {}: {reason}", name_or_path(path))
}

/// `ByteSource` is created on the viewer-open worker and moved to the UI
/// thread through the hand-off channel (design D4). This fails to compile
/// should it ever stop being `Send`.
const _: () = {
    fn assert_send<T: Send>() {}
    let _ = assert_send::<ByteSource>;
};

/// Check what exists at `path`, the resolved target of a command-line
/// `cd`/`del`/`rmdir` that is not listed in the panel, and reply with
/// `Command::PathProbed { request, kind }` (design D1/D2). The check goes
/// through the `\\?\` long-path abstraction, so a target longer than 260
/// characters is found (command-line "cd to a path longer than 260
/// characters"); an unreachable server blocks only this thread.
pub fn spawn_path_probe(request: u64, path: PathBuf, tx: Sender<Command>, token: WorkerToken) {
    spawn_path_probe_with(request, tx, token, move || probe_kind(&path));
}

/// The probe itself: `metadata` through `to_extended_path`, links followed
/// (a junction to a directory is `Dir`), and any error at all is `Missing`,
/// exactly as the reducer's old synchronous `metadata(..).ok()?` was.
pub(crate) fn probe_kind(path: &Path) -> ProbeKind {
    match std::fs::metadata(to_extended_path(path)) {
        Ok(md) if md.is_dir() => ProbeKind::Dir,
        Ok(_) => ProbeKind::File,
        Err(_) => ProbeKind::Missing,
    }
}

/// `spawn_path_probe` with the check injected (design D8): a panicked probe
/// reports `Missing`, the reply the reducer turns into "not found".
fn spawn_path_probe_with<B>(request: u64, tx: Sender<Command>, token: WorkerToken, body: B)
where
    B: FnOnce() -> ProbeKind + Send + 'static,
{
    spawn_worker(token, body, move |result| {
        let kind = result.unwrap_or_else(|msg| {
            log_worker_panic("path probe", &msg);
            ProbeKind::Missing
        });
        deliver(&tx, "path probe", Command::PathProbed { request, kind });
    });
}

/// What a viewer-open worker hands the UI thread before its `ViewerOpened`
/// reply: the request id, the path, and the open source (design D4).
/// `Command` derives `Clone`/`PartialEq`, so the source cannot travel in
/// the command itself; `Runtime` owns the channel.
pub type ViewerHandoff = (u64, PathBuf, ByteSource);

/// Open `path` for the F3 viewer off the event loop (design D4/D5). On
/// success the source goes out on `handoff` *first*, then
/// `Command::ViewerOpened { request, .. }` on `tx`, so the source is waiting
/// by the time the command is applied; on failure only
/// `Command::ViewerOpenFailed { request, .. }` is sent.
pub fn spawn_viewer_open(request: u64, path: PathBuf, handoff: Sender<ViewerHandoff>, tx: Sender<Command>, token: WorkerToken) {
    spawn_viewer_open_with(request, path, handoff, tx, token, ByteSource::open);
}

/// `spawn_viewer_open` with the open injected (design D8): a panicked open
/// reports `ViewerOpenFailed`.
fn spawn_viewer_open_with<B>(request: u64, path: PathBuf, handoff: Sender<ViewerHandoff>, tx: Sender<Command>, token: WorkerToken, body: B)
where
    B: FnOnce(&Path) -> io::Result<ByteSource> + Send + 'static,
{
    let body_path = path.clone();
    spawn_worker(
        token,
        move || body(&body_path),
        move |result| {
            let message = match result {
                Ok(Ok(source)) => {
                    let file_len = source.len();
                    if handoff.send((request, path.clone(), source)).is_err() {
                        // The runtime (and with it the event loop) is gone;
                        // the source is dropped here and nothing follows.
                        diag::log("channel", "viewer source not delivered: event loop gone");
                        return;
                    }
                    deliver(&tx, "viewer opened", Command::ViewerOpened { request, path, file_len });
                    return;
                }
                Ok(Err(e)) => cannot_message("open", &path, &e),
                Err(msg) => {
                    log_worker_panic("viewer open", &msg);
                    format!("Internal error while opening {}: {msg}", name_or_path(&path))
                }
            };
            deliver(&tx, "viewer open failure", Command::ViewerOpenFailed { request, message });
        },
    );
}

/// Open `path` for the F4 built-in editor off the event loop (design D4):
/// `EditorState::open` — the size cap, the whole-file read and the encoding
/// classification — with the same replies as before plus the request id.
pub fn spawn_editor_open(request: u64, path: PathBuf, tx: Sender<Command>, token: WorkerToken) {
    spawn_editor_open_with(request, path, tx, token, EditorState::open);
}

/// `spawn_editor_open` with the load injected (design D8): a panicked load
/// reports `EditorOpenFailed`.
fn spawn_editor_open_with<B>(request: u64, path: PathBuf, tx: Sender<Command>, token: WorkerToken, body: B)
where
    B: FnOnce(&Path) -> io::Result<LoadResult> + Send + 'static,
{
    let body_path = path.clone();
    spawn_worker(
        token,
        move || body(&body_path),
        move |result| {
            let cmd = match result {
                Ok(Ok(LoadResult::Loaded(editor))) => Command::EditorOpened { request, editor: Box::new(editor) },
                Ok(Ok(LoadResult::TooLarge { size })) => Command::EditorTooLarge { request, path, size },
                Ok(Ok(LoadResult::NotEditable { reason })) => Command::EditorNotEditable { request, path, reason },
                Ok(Err(e)) => Command::EditorOpenFailed { request, message: cannot_message("open", &path, &e) },
                Err(msg) => {
                    log_worker_panic("editor open", &msg);
                    Command::EditorOpenFailed { request, message: format!("Internal error while opening {}: {msg}", name_or_path(&path)) }
                }
            };
            deliver(&tx, "editor open", cmd);
        },
    );
}

/// Write the editor buffer `Effect::SaveEditor` carries off the event loop
/// (design D6): `EditorState::save` on the boxed copy, replying
/// `Command::EditorSaved` with the post-save state (and `then_quit`
/// unchanged) or `Command::EditorSaveFailed`. Only one save is ever in
/// flight — the reducer gates the editor on `save_pending` — so the reply
/// needs no request id.
pub fn spawn_editor_save(editor: Box<EditorState>, then_quit: bool, tx: Sender<Command>, token: WorkerToken) {
    spawn_editor_save_with(editor, then_quit, tx, token, |editor| editor.save());
}

/// `spawn_editor_save` with the write injected (design D8): a panicked save
/// reports `EditorSaveFailed`, which also lifts the editor's gate.
fn spawn_editor_save_with<B>(editor: Box<EditorState>, then_quit: bool, tx: Sender<Command>, token: WorkerToken, body: B)
where
    B: FnOnce(&mut EditorState) -> io::Result<()> + Send + 'static,
{
    let path = editor.path.clone();
    spawn_worker(
        token,
        move || {
            let mut editor = editor;
            let outcome = body(&mut editor);
            (editor, outcome)
        },
        move |result| {
            let cmd = match result {
                Ok((editor, Ok(()))) => Command::EditorSaved { editor, then_quit },
                Ok((editor, Err(e))) => Command::EditorSaveFailed { message: cannot_message("save", &editor.path, &e) },
                Err(msg) => {
                    log_worker_panic("editor save", &msg);
                    Command::EditorSaveFailed { message: format!("Internal error while saving {}: {msg}", name_or_path(&path)) }
                }
            };
            deliver(&tx, "editor save", cmd);
        },
    );
}

/// Launch `path` with its associated application off the event loop
/// (design D7): `app::open_with_association`, bracketed on Windows by
/// `CoInitializeEx(COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)` and
/// `CoUninitialize`, since shell handlers may use COM. Fire-and-forget: a
/// success sends nothing; a failure sends `OpenWithAssociationFailed` for
/// `side`, the panel the Open action was invoked on.
pub fn spawn_open_with_association(side: PanelSide, path: PathBuf, cwd: PathBuf, tx: Sender<Command>, token: WorkerToken) {
    spawn_open_with_association_with(side, path, tx, token, move |path| launch_with_association(path, &cwd));
}

/// `spawn_open_with_association` with the launch injected (design D8): a
/// panicked launch reports a failure on the invoking panel.
fn spawn_open_with_association_with<B>(side: PanelSide, path: PathBuf, tx: Sender<Command>, token: WorkerToken, body: B)
where
    B: FnOnce(&Path) -> Result<(), String> + Send + 'static,
{
    let body_path = path.clone();
    spawn_worker(
        token,
        move || body(&body_path),
        move |result| {
            let message = match result {
                Ok(Ok(())) => return,
                Ok(Err(message)) => message,
                Err(msg) => {
                    log_worker_panic("open with association", &msg);
                    format!("Could not open {} (internal error: {msg})", name_or_path(&path))
                }
            };
            deliver(&tx, "open-with failure", Command::OpenWithAssociationFailed { side, message });
        },
    );
}

/// The platform launch inside its COM bracket (Windows) or as is.
fn launch_with_association(path: &Path, cwd: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        with_com_initialized(|| crate::app::open_with_association(path, cwd))
    }
    #[cfg(not(windows))]
    {
        crate::app::open_with_association(path, cwd)
    }
}

/// Run `body` with COM initialized in a single-threaded apartment on this
/// thread (application-shell "COM is initialized on the launch worker";
/// design D7): the `COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE`
/// combination the `ShellExecute` documentation recommends.
#[cfg(windows)]
fn with_com_initialized<T>(body: impl FnOnce() -> T) -> T {
    use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE};
    com_bracket(
        // SAFETY: a null reserved pointer and a valid COINIT combination;
        // every call on this thread is paired with the `CoUninitialize`
        // below when it succeeded.
        || unsafe { CoInitializeEx(std::ptr::null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32) },
        || unsafe { CoUninitialize() },
        body,
    )
}

/// `CoInitializeEx`'s success codes: `S_OK` (initialized by this call) and
/// `S_FALSE` (already initialized on this thread, reference count bumped).
/// Both must be balanced by one `CoUninitialize`; any failure (such as
/// `RPC_E_CHANGED_MODE`) must not be.
const COM_S_OK: i32 = 0;
const COM_S_FALSE: i32 = 1;

/// The bracket itself, with the two COM calls injected so it is testable on
/// every platform: `init`, then `body`, then `uninit` only if `init`
/// succeeded.
fn com_bracket<T>(init: impl FnOnce() -> i32, uninit: impl FnOnce(), body: impl FnOnce() -> T) -> T {
    let hr = init();
    let initialized = hr == COM_S_OK || hr == COM_S_FALSE;
    let result = body();
    if initialized {
        uninit();
    }
    result
}

/// Read `path`'s immediate child directories for Tree-mode expansion
/// (`listing::list_child_dirs`) on a worker thread, reporting them back via
/// `Command::TreeNodeExpanded`. A read failure (permission denied, a
/// since-vanished directory) reports an empty child list rather than
/// aborting — `core::update`'s `TreeState::insert_children` still marks the
/// node expanded, so a broken node simply shows no children instead of
/// leaving the query outstanding forever (additional-panel-modes "Children
/// read on expand"; design D7).
pub fn spawn_tree_expand(panel: PanelSide, path: PathBuf, tx: Sender<Command>, token: WorkerToken) {
    spawn_tree_expand_with(panel, path, tx, token, |path| filecommand_core::listing::list_child_dirs(&StdFsReader, path).unwrap_or_default());
}

/// `spawn_tree_expand` with the read injected (design D6): a panicked read
/// reports no children, the same reply as a read failure.
fn spawn_tree_expand_with<B>(panel: PanelSide, path: PathBuf, tx: Sender<Command>, token: WorkerToken, body: B)
where
    B: FnOnce(&Path) -> Vec<Entry> + Send + 'static,
{
    let body_path = path.clone();
    spawn_worker(
        token,
        move || body(&body_path),
        move |result| {
            let children = result.unwrap_or_else(|msg| {
                log_worker_panic("tree expansion", &msg);
                vec![]
            });
            deliver(&tx, "tree expansion", Command::TreeNodeExpanded { panel, path, children });
        },
    );
}

/// Run the Alt+F7 subtree name search on a worker thread, streaming each
/// match back via `Command::FindFileMatch` as `listing::find_in_subtree`
/// finds it and finishing with `Command::FindFileSearchDone` — the walk
/// itself performs its own I/O per directory, so results reach the UI
/// incrementally rather than only once the whole subtree has been walked
/// (find-file "Non-blocking search with static progress", "UI stays
/// responsive during a large walk"). `cancel` is the walk's stop signal
/// (find-file "Dismissed or superseded searches stop"; listing-request-guard
/// design D5): `find_in_subtree` checks it before each directory read and
/// between entries, and once set the walk returns and no further
/// `FindFileMatch` or `FindFileSearchDone` is sent.
pub fn spawn_find_subtree(root: PathBuf, pattern: String, request: u64, cancel: CancelFlag, tx: Sender<Command>, token: WorkerToken) {
    let walk_cancel = cancel.clone();
    spawn_find_subtree_with(root, pattern, request, cancel, tx, token, move |root, pattern, tx| {
        find_in_subtree(&StdFsReader, root, pattern, &walk_cancel, |m| {
            if !walk_cancel.is_cancelled() {
                deliver(tx, "find match", Command::FindFileMatch { request, m });
            }
        });
    });
}

/// `spawn_find_subtree` with the walk injected (process-lifecycle-hardening
/// design D6): whether the walk returns or panics, the search is reported
/// done, so the find dialog never shows a search still in progress — unless
/// it was cancelled, in which case nothing is sent (listing-request-guard
/// design D5).
fn spawn_find_subtree_with<B>(root: PathBuf, pattern: String, request: u64, cancel: CancelFlag, tx: Sender<Command>, token: WorkerToken, body: B)
where
    B: FnOnce(&Path, &str, &Sender<Command>) + Send + 'static,
{
    let body_tx = tx.clone();
    spawn_worker(
        token,
        move || body(&root, &pattern, &body_tx),
        move |result| {
            if let Err(msg) = result {
                log_worker_panic("find", &msg);
            }
            if cancel.is_cancelled() {
                return;
            }
            deliver(&tx, "find done", Command::FindFileSearchDone { request });
        },
    );
}

/// Fulfil `Effect::StartListing`: stream `path`'s entries back as
/// `ListingChunk`s (a synthetic `..` first when the directory has a parent),
/// then `ListingComplete` or `ListingFailed`, every reply echoing `request`
/// so the reducer can match it against the panel's current listing
/// (panel-navigation "Superseded directory listings are discarded and
/// cancelled"; listing-request-guard design D1). `cancel` is the listing's
/// stop signal (design D5): checked before the `..` chunk, as every entry
/// arrives (inside `list_dir_streaming`, which breaks off the enumeration),
/// and before the final reply — once set, nothing further is sent, not even
/// a terminal reply, since the reducer has already superseded this id and
/// would drop it. Entries go out in batches bounded by a growing count and
/// ~50 ms of enumeration time (streaming-listing-performance design D2), so
/// the first entries and a live `Reading… N` count appear while the
/// directory is still being enumerated.
pub fn spawn_listing(panel: PanelSide, path: PathBuf, request: u64, cancel: CancelFlag, tx: Sender<Command>, token: WorkerToken) {
    spawn_listing_with(panel, path, request, cancel, tx, token, read_listing);
}

/// The listing worker's body: stream the synthetic `..` and every chunk of
/// `path`, then return the closing message — `ListingComplete`, or
/// `ListingFailed` naming the directory. `None` when a chunk could not be
/// delivered (the event loop is gone), since nothing is listening for the
/// closing message either, and `None` once `cancel` is set, since the
/// reducer has superseded this request.
fn read_listing(panel: PanelSide, path: &Path, request: u64, cancel: &CancelFlag, tx: &Sender<Command>) -> Option<Command> {
    let mut total = 0usize;

    if cancel.is_cancelled() {
        return None;
    }
    if parent_path(path).is_some() {
        total += 1;
        if !deliver(tx, "listing chunk", Command::ListingChunk { panel, entries: vec![Entry::parent_dir()], request }) {
            return None;
        }
    }

    let reader = StdFsReader;
    let result = list_dir_streaming(&reader, path, cancel, BatchPolicy::default(), &Instant::now, |chunk| {
        deliver(tx, "listing chunk", Command::ListingChunk { panel, entries: chunk, request });
    });

    if cancel.is_cancelled() {
        return None;
    }
    Some(match result {
        Ok(totals) => Command::ListingComplete { panel, total: total + totals.entries, unreadable: totals.unreadable, request },
        // The message names the directory it could not read
        // (filesystem-error-handling "Error messages name the affected
        // path"), not just the bare OS error.
        Err(e) => Command::ListingFailed { panel, message: listing_failed_message(path, &e), request },
    })
}

/// `spawn_listing` with the read injected (process-lifecycle-hardening
/// design D6): a panicked read ends as a failed listing naming the
/// directory, so the panel leaves `Reading… N` and shows the error instead
/// of waiting forever (application-shell "A panicked listing ends as a
/// failed listing"). A cancelled listing sends nothing, panicked or not.
fn spawn_listing_with<B>(panel: PanelSide, path: PathBuf, request: u64, cancel: CancelFlag, tx: Sender<Command>, token: WorkerToken, body: B)
where
    B: FnOnce(PanelSide, &Path, u64, &CancelFlag, &Sender<Command>) -> Option<Command> + Send + 'static,
{
    let body_tx = tx.clone();
    let body_path = path.clone();
    let body_cancel = cancel.clone();
    spawn_worker(
        token,
        move || body(panel, &body_path, request, &body_cancel, &body_tx),
        move |result| {
            let closing = match result {
                Ok(closing) => closing,
                Err(msg) => {
                    log_worker_panic("listing", &msg);
                    if cancel.is_cancelled() {
                        None
                    } else {
                        Some(Command::ListingFailed { panel, message: format!("Internal error while reading {}: {msg}", path.display()), request })
                    }
                }
            };
            if let Some(cmd) = closing {
                let kind = if matches!(cmd, Command::ListingFailed { .. }) { "listing failure" } else { "listing complete" };
                deliver(&tx, kind, cmd);
            }
        },
    );
}

/// The main loop's answer to a conflict/error dialog the worker thread is
/// blocked waiting on.
pub enum JobReply {
    Conflict(ConflictChoice),
    Error(ErrorChoice),
}

/// What the main loop keeps for an in-flight job: a flag it can flip to
/// request cancellation, a channel to forward dialog answers down to the
/// blocked worker thread, and the thread itself so quit can wait for its
/// cancellation cleanup (application-shell "Quit waits for job cancellation
/// cleanup"; process-lifecycle-hardening design D3). Not part of
/// `core::State` — channels/flags/threads are TUI-only plumbing, not pure
/// application state.
pub struct JobHandle {
    pub cancel: CancelFlag,
    pub reply_tx: Sender<JobReply>,
    pub thread: JoinHandle<()>,
}

/// How often a job waiting on a conflict or error answer re-checks its
/// cancel flag (design D5). Comfortably inside the 2 s quit budget.
const REPLY_POLL: Duration = Duration::from_millis(100);

struct ChannelSink {
    tx: Sender<Command>,
    reply_rx: mpsc::Receiver<JobReply>,
    cancel: CancelFlag,
    source_dir: PathBuf,
    dest_dir: PathBuf,
    /// Rate-limits `progress` to ~20 updates a second (idle-aware-event-loop
    /// "Progress updates are rate-limited"; design D7) — at the sink rather
    /// than inside the engine, so every emitter (per-chunk copy progress,
    /// per-file delete/move progress) is throttled in one place and
    /// `run_job`'s own tests stay unthrottled and deterministic.
    throttle: ProgressThrottle,
}

impl ChannelSink {
    /// Design D7's flush points: the newest held progress goes out ahead of
    /// any message the user makes a decision on — or that ends the job — so
    /// those moments never render stale file names or counts.
    fn flush_progress(&mut self) {
        if let Some(info) = self.throttle.flush() {
            deliver(&self.tx, "job progress", Command::JobProgress(info));
        }
    }

    /// Wait for the main loop's answer to the prompt just sent, observing
    /// cancellation meanwhile (operation-dialogs "Cancellation is observed
    /// while a job waits for an answer"; design D5): `None` when the cancel
    /// flag was set before an answer arrived, or when the answer can no
    /// longer arrive because the `JobHandle` — and its sender — is gone.
    /// Either way the caller resolves the prompt as an abort, never as a
    /// default Skip applied to this and later items.
    fn await_reply(&self) -> Option<JobReply> {
        loop {
            match self.reply_rx.recv_timeout(REPLY_POLL) {
                Ok(reply) => return Some(reply),
                Err(RecvTimeoutError::Timeout) => {
                    if self.cancel.is_cancelled() {
                        return None;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return None,
            }
        }
    }
}

impl JobSink for ChannelSink {
    fn progress(&mut self, info: ProgressInfo) {
        if let Some(info) = self.throttle.offer(info, Instant::now()) {
            deliver(&self.tx, "job progress", Command::JobProgress(info));
        }
    }

    fn conflict(&mut self, info: ConflictInfo) -> ConflictChoice {
        self.flush_progress();
        // An undeliverable prompt means the event loop is gone: stop rather
        // than continue unattended with a default answer.
        if !deliver(&self.tx, "conflict prompt", Command::JobConflict(info)) {
            return ConflictChoice::Cancel;
        }
        match self.await_reply() {
            Some(JobReply::Conflict(choice)) => choice,
            // Cancelled or disconnected while waiting — or a reply of the
            // wrong kind, which core never sends while a conflict is shown.
            _ => ConflictChoice::Cancel,
        }
    }

    fn error(&mut self, info: ErrorInfo) -> ErrorChoice {
        self.flush_progress();
        if !deliver(&self.tx, "error prompt", Command::JobError(info)) {
            return ErrorChoice::Abort;
        }
        match self.await_reply() {
            Some(JobReply::Error(choice)) => choice,
            _ => ErrorChoice::Abort,
        }
    }

    fn done(&mut self, outcome: JobOutcome) {
        self.flush_progress();
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
    use filecommand_core::editor::NotEditableReason;
    use filecommand_core::info::PENDING;
    use filecommand_core::listing::{RawDirEntry, RawDirListing};
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
        spawn_viewer_search(path, 0, b"brown".to_vec(), 7, CancelFlag::new(), tx, WorkerGauge::new().token());
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
        spawn_viewer_search(path, 0, b"zebra".to_vec(), 1, CancelFlag::new(), tx, WorkerGauge::new().token());
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Command::ViewerSearchResult { offset: None, match_range: None, request: 1 }) => {}
            other => panic!("expected a no-match ViewerSearchResult, got {other:?}"),
        }
    }

    #[test]
    fn spawn_viewer_search_on_an_unopenable_path_reports_no_match_instead_of_panicking() {
        let missing = std::env::temp_dir().join("filecommand-tui-worker-viewer-test-does-not-exist.bin");
        let (tx, rx) = mpsc::channel();
        spawn_viewer_search(missing, 0, b"x".to_vec(), 1, CancelFlag::new(), tx, WorkerGauge::new().token());
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Command::ViewerSearchResult { offset: None, match_range: None, request: 1 }) => {}
            other => panic!("expected a no-match ViewerSearchResult, got {other:?}"),
        }
    }

    /// viewer: Instant open — "Closing the viewer releases the file": the
    /// worker's `ByteSource` is a local that drops before the result is
    /// delivered, so once a search with no match has scanned to the end of
    /// the file, no FileCommand thread still holds it — the file and its
    /// directory can be removed (an open handle blocks directory removal on
    /// Windows). Cancelling a still-running search when the viewer closes
    /// is `listing-request-guard`'s scope; this pins the release-on-return
    /// half.
    #[test]
    fn spawn_viewer_search_releases_the_file_when_the_search_finishes() {
        // Larger than one chunk and match-free, so the worker scans to the
        // end of the file before returning.
        let contents = vec![b'y'; 3 * DEFAULT_CHUNK_SIZE];
        let path = temp_file("release", &contents);
        let dir = path.parent().unwrap().to_path_buf();
        let (tx, rx) = mpsc::channel();
        spawn_viewer_search(path.clone(), 0, b"zebra".to_vec(), 1, CancelFlag::new(), tx, WorkerGauge::new().token());
        match rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(Command::ViewerSearchResult { offset: None, match_range: None, request: 1 }) => {}
            other => panic!("expected a no-match ViewerSearchResult, got {other:?}"),
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    struct FakeReader(io::Result<RawDirListing>);

    impl FsReader for FakeReader {
        fn read_dir(&self, _path: &Path) -> io::Result<RawDirListing> {
            match &self.0 {
                Ok(listing) => Ok(listing.clone()),
                Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
            }
        }
    }

    fn raw(name: &str, is_dir: bool) -> RawDirEntry {
        RawDirEntry { name: name.into(), is_dir, size: 0, modified: None, hidden: false, link: false }
    }

    #[test]
    fn gather_info_counts_files_and_directories_separately() {
        let reader = FakeReader(Ok(RawDirListing { entries: vec![raw("a.txt", false), raw("b.txt", false), raw("sub", true)], unreadable: 0 }));
        let values = gather_info(&reader, Path::new(r"C:\somewhere"));
        assert_eq!(values.file_count, Some(2));
        assert_eq!(values.dir_count, Some(1));
    }

    #[test]
    fn gather_info_counts_directory_links_as_directories_and_ignores_skipped_entries() {
        // link-aware-listing design D6 (task 4.11): a junction to a
        // directory is a directory for the Info counts, as it is for the
        // panel, and entries the reader skipped as unreadable count as
        // neither.
        let dir_link = RawDirEntry { name: "lnk".into(), is_dir: true, size: 0, modified: None, hidden: false, link: true };
        let file_link = RawDirEntry { name: "f.lnk".into(), is_dir: false, size: 7, modified: None, hidden: false, link: true };
        let reader = FakeReader(Ok(RawDirListing { entries: vec![raw("a.txt", false), raw("sub", true), dir_link, file_link], unreadable: 3 }));
        let values = gather_info(&reader, Path::new(r"C:\somewhere"));
        assert_eq!(values.file_count, Some(2));
        assert_eq!(values.dir_count, Some(2));
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

        slot.request_with(PanelSide::Left, PathBuf::from("/first"), 1, tx.clone(), WorkerGauge::new().token(), timeout, fake_query(&fake));
        assert_eq!(started_rx.recv_timeout(std::time::Duration::from_secs(5)), Ok(PathBuf::from("/first")), "the first query starts");
        slot.request_with(PanelSide::Left, PathBuf::from("/second"), 2, tx.clone(), WorkerGauge::new().token(), timeout, fake_query(&fake));
        slot.request_with(PanelSide::Left, PathBuf::from("/third"), 3, tx.clone(), WorkerGauge::new().token(), timeout, fake_query(&fake));
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
        let fake = FakeGitQuery::new(started_tx, vec![(PathBuf::from("/repo/src"), FakeScript::PublishThenBlock(PathBuf::from("/repo"), gate_rx))]);
        let slot = GitQuerySlot::new();
        let (tx, rx) = mpsc::channel();
        let timeout = std::time::Duration::from_millis(300);

        slot.request_with(PanelSide::Left, PathBuf::from("/repo/src"), 1, tx.clone(), WorkerGauge::new().token(), timeout, fake_query(&fake));
        assert_eq!(started_rx.recv_timeout(std::time::Duration::from_secs(5)), Ok(PathBuf::from("/repo/src")));
        // Queued while the first query is still walking (before it times
        // out), inside the about-to-be-remembered repository.
        slot.request_with(PanelSide::Left, PathBuf::from("/repo/docs"), 2, tx, WorkerGauge::new().token(), timeout, fake_query(&fake));

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
        let fake = FakeGitQuery::new(started_tx, vec![(PathBuf::from("/repo/src"), FakeScript::PublishThenBlock(PathBuf::from("/repo"), gate_rx))]);
        let slot = GitQuerySlot::new();
        let (tx, rx) = mpsc::channel();
        slot.request_with(
            PanelSide::Left,
            PathBuf::from("/repo/src"),
            1,
            tx,
            WorkerGauge::new().token(),
            std::time::Duration::from_millis(100),
            fake_query(&fake),
        );

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
        slot.request_with(
            PanelSide::Left,
            PathBuf::from("/slow/deep"),
            1,
            tx,
            WorkerGauge::new().token(),
            std::time::Duration::from_millis(100),
            fake_query(&fake),
        );

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

        slot.request_with(PanelSide::Left, PathBuf::from("/panic"), 1, tx.clone(), WorkerGauge::new().token(), timeout, fake_query(&fake));
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

        slot.request_with(PanelSide::Left, PathBuf::from("/ok"), 2, tx, WorkerGauge::new().token(), timeout, fake_query(&fake));
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
        spawn_listing(PanelSide::Left, dir, 7, CancelFlag::new(), tx, WorkerGauge::new().token());
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(5)) {
                Ok(Command::ListingChunk { .. }) => continue, // the synthetic `..` entry
                Ok(Command::ListingFailed { panel, message, .. }) => {
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
        spawn_find_subtree(dir.clone(), "report".to_string(), 7, CancelFlag::new(), tx, WorkerGauge::new().token());

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

    // -------------------------------------------------------------
    // listing-request-guard: workers echo the request id and go silent
    // once cancelled (design D1/D5; task 5.13)
    // -------------------------------------------------------------

    fn listing_fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("filecommand-tui-worker-guard-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("report.txt"), b"x").unwrap();
        std::fs::write(dir.join("other.txt"), b"x").unwrap();
        dir
    }

    /// A cancelled worker sends nothing: the first thing the receiver sees
    /// is the channel closing when the thread returns — never a message.
    fn assert_silent(rx: &mpsc::Receiver<Command>) {
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Err(RecvTimeoutError::Disconnected) | Err(RecvTimeoutError::Timeout) => {}
            Ok(msg) => panic!("a cancelled worker must send nothing, got {msg:?}"),
        }
    }

    #[test]
    fn spawn_listing_echoes_the_request_id_on_every_reply() {
        let dir = listing_fixture("echo");
        let (tx, rx) = mpsc::channel();
        spawn_listing(PanelSide::Left, dir.clone(), 42, CancelFlag::new(), tx, WorkerGauge::new().token());
        let mut chunks = 0;
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(5)) {
                Ok(Command::ListingChunk { panel: PanelSide::Left, request, .. }) => {
                    assert_eq!(request, 42);
                    chunks += 1;
                }
                Ok(Command::ListingComplete { panel: PanelSide::Left, total, unreadable, request }) => {
                    assert_eq!(request, 42);
                    assert_eq!(total, 4, "`..` plus three entries");
                    assert_eq!(unreadable, 0);
                    break;
                }
                other => panic!("unexpected message: {other:?}"),
            }
        }
        assert!(chunks >= 2, "the synthetic `..` chunk and at least one directory chunk");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spawn_listing_with_a_pre_cancelled_flag_sends_nothing() {
        let dir = listing_fixture("cancelled");
        let (tx, rx) = mpsc::channel();
        let cancel = CancelFlag::new();
        cancel.cancel();
        spawn_listing(PanelSide::Left, dir.clone(), 7, cancel, tx, WorkerGauge::new().token());
        assert_silent(&rx);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spawn_find_subtree_with_a_pre_cancelled_flag_sends_nothing() {
        let dir = listing_fixture("find-cancelled");
        let (tx, rx) = mpsc::channel();
        let cancel = CancelFlag::new();
        cancel.cancel();
        spawn_find_subtree(dir.clone(), "report".to_string(), 7, cancel, tx, WorkerGauge::new().token());
        assert_silent(&rx);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spawn_viewer_search_with_a_pre_cancelled_flag_sends_nothing() {
        let path = temp_file("search-cancelled", b"the quick brown fox");
        let (tx, rx) = mpsc::channel();
        let cancel = CancelFlag::new();
        cancel.cancel();
        spawn_viewer_search(path, 0, b"brown".to_vec(), 7, cancel, tx, WorkerGauge::new().token());
        assert_silent(&rx);
    }

    // -------------------------------------------------------------
    // idle-aware-event-loop: the worker gauge (design D3; task 2.1) and
    // the sink's progress throttle (file-operations "Progress updates are
    // rate-limited"; design D7; task 4.2)
    // -------------------------------------------------------------

    #[test]
    fn the_gauge_counts_live_tokens_and_a_panicked_thread_releases_its() {
        let gauge = WorkerGauge::new();
        assert_eq!(gauge.live(), 0);
        let token = gauge.token();
        assert_eq!(gauge.live(), 1);
        let panicked = std::thread::spawn(move || {
            let _token = token;
            panic!("scripted worker panic");
        })
        .join();
        assert!(panicked.is_err());
        assert_eq!(gauge.live(), 0, "the token's Drop decrements even on panic");
    }

    /// A `ChannelSink` over a recording channel, with the reply channel's
    /// sender returned so conflict/error answers can be pre-loaded.
    fn recording_sink() -> (ChannelSink, Sender<JobReply>, mpsc::Receiver<Command>) {
        let (tx, rx) = mpsc::channel();
        let (reply_tx, reply_rx) = mpsc::channel();
        let sink = ChannelSink {
            tx,
            reply_rx,
            cancel: CancelFlag::new(),
            source_dir: PathBuf::from("/src"),
            dest_dir: PathBuf::from("/dst"),
            throttle: ProgressThrottle::new(),
        };
        (sink, reply_tx, rx)
    }

    fn progress(files_done: usize, bytes_done: u64, name: &str) -> ProgressInfo {
        ProgressInfo { files_done, files_total: 3, bytes_done, bytes_total: 6_000, current_file: std::ffi::OsString::from(name) }
    }

    #[test]
    fn a_fast_job_flushes_its_final_progress_before_done() {
        // file-operations "The final progress is always delivered": three
        // files' progress inside one 50 ms interval is held by the throttle,
        // and `done` pushes the newest figures ahead of `JobDone`.
        let (mut sink, _reply_tx, rx) = recording_sink();
        sink.progress(progress(1, 2_000, "a.txt"));
        sink.progress(progress(2, 4_000, "b.txt"));
        sink.progress(progress(3, 6_000, "c.txt"));
        sink.done(JobOutcome::Completed { skipped: vec![] });

        let msgs: Vec<Command> = rx.try_iter().collect();
        assert_eq!(
            msgs,
            vec![
                Command::JobProgress(progress(1, 2_000, "a.txt")),
                Command::JobProgress(progress(3, 6_000, "c.txt")),
                Command::JobDone {
                    outcome: JobOutcome::Completed { skipped: vec![] },
                    source_dir: PathBuf::from("/src"),
                    dest_dir: PathBuf::from("/dst")
                },
            ],
            "the first update goes straight out, the newest held one precedes JobDone, the middle one is dropped"
        );
    }

    #[test]
    fn a_conflict_is_preceded_by_the_pending_progress() {
        // file-operations "Progress is current when a conflict is asked":
        // the held `a.txt` update reaches the UI ahead of the question.
        let (mut sink, reply_tx, rx) = recording_sink();
        sink.progress(progress(1, 2_000, "a.txt"));
        sink.progress(progress(2, 4_000, "b.txt"));
        let conflict = ConflictInfo {
            source_name: std::ffi::OsString::from("b.txt"),
            source_size: 2_000,
            source_modified: None,
            target_path: PathBuf::from("/dst/b.txt"),
            target_size: 1_000,
            target_modified: None,
        };
        // Pre-load the answer so the blocking `conflict` returns at once.
        reply_tx.send(JobReply::Conflict(ConflictChoice::Skip)).unwrap();
        assert_eq!(sink.conflict(conflict.clone()), ConflictChoice::Skip);

        let msgs: Vec<Command> = rx.try_iter().collect();
        assert_eq!(
            msgs,
            vec![
                Command::JobProgress(progress(1, 2_000, "a.txt")),
                Command::JobProgress(progress(2, 4_000, "b.txt")),
                Command::JobConflict(conflict),
            ],
            "the held update is flushed before the conflict question"
        );
    }

    // -------------------------------------------------------------
    // process-lifecycle-hardening: cancellation is observed while a job
    // waits for an answer (operation-dialogs; design D5; task 3.2)
    // -------------------------------------------------------------

    fn sample_conflict() -> ConflictInfo {
        ConflictInfo {
            source_name: std::ffi::OsString::from("b.txt"),
            source_size: 2_000,
            source_modified: None,
            target_path: PathBuf::from("/dst/b.txt"),
            target_size: 1_000,
            target_modified: None,
        }
    }

    fn sample_error() -> ErrorInfo {
        ErrorInfo { path: PathBuf::from("/src/locked.db"), message: "in use".to_string() }
    }

    /// Set the sink's cancel flag from another thread after `delay`.
    fn cancel_after(cancel: &CancelFlag, delay: Duration) {
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            cancel.cancel();
        });
    }

    #[test]
    fn a_cancel_request_while_waiting_on_a_conflict_resolves_it_as_cancel() {
        // Scenario "A cancel request ends a job waiting on a conflict
        // prompt": no answer is ever sent; the flag alone ends the wait,
        // within about two poll intervals.
        let (mut sink, _reply_tx, _rx) = recording_sink();
        cancel_after(&sink.cancel, Duration::from_millis(50));
        let started = Instant::now();
        assert_eq!(sink.conflict(sample_conflict()), ConflictChoice::Cancel);
        assert!(started.elapsed() < Duration::from_millis(500), "the wait ends promptly after the flag is set, took {:?}", started.elapsed());
    }

    #[test]
    fn a_cancel_request_while_waiting_on_an_error_resolves_it_as_abort() {
        // Scenario "A cancel request ends a job waiting on an error prompt".
        let (mut sink, _reply_tx, _rx) = recording_sink();
        cancel_after(&sink.cancel, Duration::from_millis(50));
        let started = Instant::now();
        assert_eq!(sink.error(sample_error()), ErrorChoice::Abort);
        assert!(started.elapsed() < Duration::from_millis(500), "took {:?}", started.elapsed());
    }

    #[test]
    fn a_dropped_reply_sender_resolves_a_conflict_as_cancel_and_an_error_as_abort() {
        // Scenario "A lost answer does not resume the job with a default":
        // the `JobHandle` is gone, so neither prompt can ever be answered.
        let (mut sink, reply_tx, _rx) = recording_sink();
        drop(reply_tx);
        assert_eq!(sink.conflict(sample_conflict()), ConflictChoice::Cancel, "not the old default Skip");
        assert_eq!(sink.error(sample_error()), ErrorChoice::Abort);
    }

    #[test]
    fn a_normal_answer_is_still_returned_unchanged() {
        let (mut sink, reply_tx, _rx) = recording_sink();
        reply_tx.send(JobReply::Conflict(ConflictChoice::Rename(std::ffi::OsString::from("b2.txt")))).unwrap();
        assert_eq!(sink.conflict(sample_conflict()), ConflictChoice::Rename(std::ffi::OsString::from("b2.txt")));
        reply_tx.send(JobReply::Error(ErrorChoice::Retry)).unwrap();
        assert_eq!(sink.error(sample_error()), ErrorChoice::Retry);
    }

    #[test]
    fn an_answer_arriving_after_a_poll_interval_is_still_delivered() {
        // The timeout loop keeps waiting while the flag is clear: an answer
        // that takes longer than one poll interval still arrives.
        let (mut sink, reply_tx, _rx) = recording_sink();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            let _ = reply_tx.send(JobReply::Conflict(ConflictChoice::Overwrite));
        });
        assert_eq!(sink.conflict(sample_conflict()), ConflictChoice::Overwrite);
    }

    // -------------------------------------------------------------
    // process-lifecycle-hardening: worker panics become each worker's
    // defined failure reply (application-shell "Panic hook restores the
    // terminal before reporting"; design D6; task 5.4)
    // -------------------------------------------------------------

    #[test]
    fn guarded_returns_the_value_or_the_panic_message() {
        assert_eq!(guarded(|| 7), Ok(7));
        assert_eq!(guarded::<()>(|| panic!("plain str")), Err("plain str".to_string()));
        let n = 3;
        assert_eq!(guarded::<()>(|| panic!("formatted {n}")), Err("formatted 3".to_string()));
        assert_eq!(guarded::<()>(|| std::panic::panic_any(42u8)), Err("unknown panic".to_string()));
    }

    fn recv(rx: &mpsc::Receiver<Command>) -> Command {
        rx.recv_timeout(Duration::from_secs(5)).expect("the worker replies even after a panic")
    }

    // -------------------------------------------------------------
    // offload-blocking-io: the command-line probe, the F3/F4 opens, the
    // editor save and the launch worker (tasks 5.6, 5.7, 5.8; design D1,
    // D2, D4, D6, D7, D8)
    // -------------------------------------------------------------

    /// A fresh, empty per-test directory under the system temp dir.
    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("filecommand-tui-worker-offload-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn probe(path: PathBuf, request: u64) -> ProbeKind {
        let (tx, rx) = mpsc::channel();
        spawn_path_probe(request, path, tx, WorkerGauge::new().token());
        match recv(&rx) {
            Command::PathProbed { request: got, kind } => {
                assert_eq!(got, request, "the request id is echoed back unchanged");
                kind
            }
            other => panic!("expected PathProbed, got {other:?}"),
        }
    }

    /// Design D2: a directory, a file and a missing path map to the three
    /// kinds.
    #[test]
    fn spawn_path_probe_reports_dir_file_and_missing() {
        let dir = scratch_dir("probe");
        let file = dir.join("notes.txt");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(probe(dir.clone(), 1), ProbeKind::Dir);
        assert_eq!(probe(file, 2), ProbeKind::File);
        assert_eq!(probe(dir.join("nope"), 3), ProbeKind::Missing);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// command-line "cd to a path longer than 260 characters": the probe
    /// goes through `to_extended_path`, so a directory whose absolute path
    /// exceeds `MAX_PATH` is found rather than reported missing.
    #[cfg(windows)]
    #[test]
    fn spawn_path_probe_finds_a_directory_whose_path_exceeds_260_characters() {
        let root = scratch_dir("long");
        let deep = root.join("a".repeat(120)).join("b".repeat(120)).join("c".repeat(60));
        assert!(deep.as_os_str().len() > 260, "precondition: {} characters", deep.as_os_str().len());
        std::fs::create_dir_all(to_extended_path(&deep)).expect("created through the \\\\?\\ prefix");
        assert_eq!(probe(deep, 9), ProbeKind::Dir);
        let _ = std::fs::remove_dir_all(to_extended_path(&root));
    }

    /// Design D8: a panicked probe reports `Missing`, with the request id.
    #[test]
    fn a_panicked_probe_reports_missing() {
        let (tx, rx) = mpsc::channel();
        spawn_path_probe_with(4, tx, WorkerGauge::new().token(), || panic!("scripted probe panic"));
        assert_eq!(recv(&rx), Command::PathProbed { request: 4, kind: ProbeKind::Missing });
    }

    /// Design D4: the source is on the hand-off channel before the
    /// `ViewerOpened` reply is sent, under the same request id.
    #[test]
    fn spawn_viewer_open_hands_the_source_over_before_viewer_opened() {
        let path = temp_file("viewer-open", b"hello world");
        let (tx, rx) = mpsc::channel();
        let (handoff_tx, handoff_rx) = mpsc::channel();
        spawn_viewer_open(4, path.clone(), handoff_tx, tx, WorkerGauge::new().token());
        match recv(&rx) {
            Command::ViewerOpened { request: 4, path: opened, file_len: 11 } => assert_eq!(opened, path),
            other => panic!("expected ViewerOpened, got {other:?}"),
        }
        let (request, handed_path, source) = handoff_rx.try_recv().expect("the source was queued before the command");
        assert_eq!(request, 4);
        assert_eq!(handed_path, path);
        assert_eq!(source.len(), 11);
    }

    #[test]
    fn spawn_viewer_open_on_a_missing_file_reports_viewer_open_failed() {
        let missing = std::env::temp_dir().join("filecommand-tui-worker-offload-missing.bin");
        let (tx, rx) = mpsc::channel();
        let (handoff_tx, handoff_rx) = mpsc::channel();
        spawn_viewer_open(5, missing, handoff_tx, tx, WorkerGauge::new().token());
        match recv(&rx) {
            Command::ViewerOpenFailed { request: 5, message } => {
                assert!(message.starts_with("Cannot open filecommand-tui-worker-offload-missing.bin:"), "{message}");
            }
            other => panic!("expected ViewerOpenFailed, got {other:?}"),
        }
        assert!(handoff_rx.try_recv().is_err(), "nothing is handed over on failure");
    }

    /// Design D8: a panicked open reports `ViewerOpenFailed`.
    #[test]
    fn a_panicked_viewer_open_reports_viewer_open_failed() {
        let (tx, rx) = mpsc::channel();
        let (handoff_tx, _handoff_rx) = mpsc::channel();
        spawn_viewer_open_with(6, PathBuf::from("/x/slow.log"), handoff_tx, tx, WorkerGauge::new().token(), |_| panic!("scripted open panic"));
        match recv(&rx) {
            Command::ViewerOpenFailed { request: 6, message } => assert_eq!(message, "Internal error while opening slow.log: scripted open panic"),
            other => panic!("expected ViewerOpenFailed, got {other:?}"),
        }
    }

    /// Design D4: `spawn_editor_open` wraps `EditorState::open` with the
    /// same replies as before plus the request id.
    #[test]
    fn spawn_editor_open_reports_loaded_not_editable_too_large_and_failed() {
        let dir = scratch_dir("editor-open");
        let text = dir.join("notes.txt");
        std::fs::write(&text, b"abc\n").unwrap();
        let (tx, rx) = mpsc::channel();
        spawn_editor_open(8, text.clone(), tx, WorkerGauge::new().token());
        match recv(&rx) {
            Command::EditorOpened { request: 8, editor } => {
                assert_eq!(editor.path, text);
                assert_eq!(editor.lines, vec!["abc".to_string(), String::new()]);
            }
            other => panic!("expected EditorOpened, got {other:?}"),
        }

        let binary = dir.join("tool.exe");
        std::fs::write(&binary, b"MZ\0\0").unwrap();
        let (tx, rx) = mpsc::channel();
        spawn_editor_open(9, binary.clone(), tx, WorkerGauge::new().token());
        match recv(&rx) {
            Command::EditorNotEditable { request: 9, path, reason: NotEditableReason::Binary } => assert_eq!(path, binary),
            other => panic!("expected EditorNotEditable, got {other:?}"),
        }

        let (tx, rx) = mpsc::channel();
        spawn_editor_open_with(10, dir.join("huge.log"), tx, WorkerGauge::new().token(), |_| Ok(LoadResult::TooLarge { size: 20_000_000 }));
        assert_eq!(recv(&rx), Command::EditorTooLarge { request: 10, path: dir.join("huge.log"), size: 20_000_000 });

        let (tx, rx) = mpsc::channel();
        spawn_editor_open(11, dir.join("nope.txt"), tx, WorkerGauge::new().token());
        match recv(&rx) {
            Command::EditorOpenFailed { request: 11, message } => assert!(message.starts_with("Cannot open nope.txt:"), "{message}"),
            other => panic!("expected EditorOpenFailed, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Design D8: a panicked load reports `EditorOpenFailed`.
    #[test]
    fn a_panicked_editor_open_reports_editor_open_failed() {
        let (tx, rx) = mpsc::channel();
        spawn_editor_open_with(12, PathBuf::from("/x/notes.txt"), tx, WorkerGauge::new().token(), |_| panic!("scripted load panic"));
        match recv(&rx) {
            Command::EditorOpenFailed { request: 12, message } => assert_eq!(message, "Internal error while opening notes.txt: scripted load panic"),
            other => panic!("expected EditorOpenFailed, got {other:?}"),
        }
    }

    /// Design D6: the save runs on the worker and replies with the post-save
    /// state and `then_quit` unchanged.
    #[test]
    fn spawn_editor_save_writes_the_file_and_replies_with_the_saved_state() {
        let dir = scratch_dir("editor-save");
        let path = dir.join("notes.txt");
        std::fs::write(&path, b"abc\n").unwrap();
        let mut editor = EditorState::from_bytes(path.clone(), b"abc\n").unwrap();
        editor.type_char('X');
        let (tx, rx) = mpsc::channel();
        spawn_editor_save(Box::new(editor), true, tx, WorkerGauge::new().token());
        match recv(&rx) {
            Command::EditorSaved { editor, then_quit: true } => {
                assert!(!editor.is_modified(), "the reply carries the clean post-save state");
                assert_eq!(editor.path, path);
            }
            other => panic!("expected EditorSaved, got {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "Xabc\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A failed or panicked save reports `EditorSaveFailed` naming the file
    /// (design D6, D8).
    #[test]
    fn a_failed_or_panicked_editor_save_reports_editor_save_failed() {
        let editor = EditorState::from_bytes(PathBuf::from("/x/edit.txt"), b"abc\n").unwrap();
        let (tx, rx) = mpsc::channel();
        spawn_editor_save_with(Box::new(editor.clone()), false, tx, WorkerGauge::new().token(), |_| {
            Err(io::Error::new(io::ErrorKind::StorageFull, "disk full"))
        });
        assert_eq!(recv(&rx), Command::EditorSaveFailed { message: "Cannot save edit.txt: disk full".to_string() });

        let (tx, rx) = mpsc::channel();
        spawn_editor_save_with(Box::new(editor), false, tx, WorkerGauge::new().token(), |_| panic!("scripted save panic"));
        assert_eq!(recv(&rx), Command::EditorSaveFailed { message: "Internal error while saving edit.txt: scripted save panic".to_string() });
    }

    /// Design D7: `CoUninitialize` is called only after an initialization
    /// that returned `S_OK` or `S_FALSE`, and always after the body.
    #[test]
    fn com_bracket_uninitializes_only_after_a_successful_initialization() {
        use std::cell::RefCell;
        const RPC_E_CHANGED_MODE: i32 = 0x8001_0106_u32 as i32;
        for (hr, expect_uninit) in [(COM_S_OK, true), (COM_S_FALSE, true), (RPC_E_CHANGED_MODE, false), (-1, false)] {
            let log = RefCell::new(Vec::new());
            let out = com_bracket(
                || {
                    log.borrow_mut().push("init");
                    hr
                },
                || log.borrow_mut().push("uninit"),
                || {
                    log.borrow_mut().push("body");
                    42
                },
            );
            assert_eq!(out, 42, "the body's value comes back");
            let expected: Vec<&str> = if expect_uninit { vec!["init", "body", "uninit"] } else { vec!["init", "body"] };
            assert_eq!(*log.borrow(), expected, "hr = {hr:#x}");
        }
    }

    /// application-shell "Launching a slow handler leaves the panels
    /// interactive": the failure carries the invoking side. A missing file
    /// never launches anything, so the real `ShellExecuteW` path is safe to
    /// exercise here.
    #[cfg(windows)]
    #[test]
    fn spawn_open_with_association_on_a_missing_file_reports_the_failure_on_the_original_side() {
        let dir = scratch_dir("open-with");
        let missing = dir.join("missing-report.pdf");
        let (tx, rx) = mpsc::channel();
        spawn_open_with_association(PanelSide::Right, missing, dir.clone(), tx, WorkerGauge::new().token());
        match recv(&rx) {
            Command::OpenWithAssociationFailed { side: PanelSide::Right, message } => {
                assert!(message.contains("missing-report.pdf"), "the failure names the target: {message}");
            }
            other => panic!("expected OpenWithAssociationFailed for the right panel, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A successful launch sends nothing (fire-and-forget); a panicked one
    /// reports a failure on the invoking panel (design D7, D8).
    #[test]
    fn open_with_association_worker_is_silent_on_success_and_reports_a_panic() {
        let (tx, rx) = mpsc::channel();
        spawn_open_with_association_with(PanelSide::Left, PathBuf::from("/x/report.pdf"), tx, WorkerGauge::new().token(), |_| Ok(()));
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "a successful launch reports nothing");

        let (tx, rx) = mpsc::channel();
        spawn_open_with_association_with(PanelSide::Left, PathBuf::from("/x/report.pdf"), tx, WorkerGauge::new().token(), |_| {
            panic!("scripted launch panic")
        });
        assert_eq!(
            recv(&rx),
            Command::OpenWithAssociationFailed {
                side: PanelSide::Left,
                message: "Could not open report.pdf (internal error: scripted launch panic)".to_string()
            }
        );
    }

    #[test]
    fn a_panicked_listing_ends_as_a_failed_listing_naming_the_path() {
        let (tx, rx) = mpsc::channel();
        let gauge = WorkerGauge::new();
        let dir = PathBuf::from(if cfg!(windows) { r"C:\data\reports" } else { "/data/reports" });
        spawn_listing_with(PanelSide::Right, dir.clone(), 7, CancelFlag::new(), tx, gauge.token(), |_, _, _, _, _| panic!("scripted listing panic"));
        match recv(&rx) {
            Command::ListingFailed { panel, message, request } => {
                assert_eq!(panel, PanelSide::Right);
                assert_eq!(request, 7, "the failure echoes the listing request id");
                assert_eq!(message, format!("Internal error while reading {}: scripted listing panic", dir.display()));
            }
            other => panic!("expected ListingFailed, got {other:?}"),
        }
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err(), "nothing else follows");
    }

    #[test]
    fn a_panicked_job_ends_cancelled_with_an_internal_error_item() {
        let (tx, rx) = mpsc::channel();
        let job = Job {
            kind: filecommand_core::fs_ops::JobKind::Copy,
            sources: vec![],
            source_dir: PathBuf::from("/src"),
            dest_dir: PathBuf::from("/dst"),
            new_dir_name: None,
        };
        let handle = spawn_job_with(job, tx, WorkerGauge::new().token(), |_, _| panic!("scripted job panic"));
        match recv(&rx) {
            Command::JobDone { outcome: JobOutcome::Cancelled { skipped }, source_dir, dest_dir } => {
                assert_eq!((source_dir, dest_dir), (PathBuf::from("/src"), PathBuf::from("/dst")));
                assert_eq!(skipped.len(), 1);
                assert_eq!(skipped[0].path, PathBuf::from("/src"));
                assert_eq!(skipped[0].reason, "internal error: scripted job panic");
                assert_eq!(skipped[0].kind, ItemKind::Skipped);
            }
            other => panic!("expected a Cancelled JobDone, got {other:?}"),
        }
        assert!(handle.thread.join().is_ok(), "the panic was contained: the thread itself exits normally");
    }

    #[test]
    fn a_panicked_info_query_resolves_every_value() {
        let (tx, rx) = mpsc::channel();
        spawn_info_query_with(PanelSide::Left, PathBuf::from("/x"), 4, tx, WorkerGauge::new().token(), |_| panic!("scripted info panic"));
        match recv(&rx) {
            Command::InfoResolved { panel, path, request, values } => {
                assert_eq!((panel, path, request), (PanelSide::Left, PathBuf::from("/x"), 4));
                assert!(values.is_complete(), "no field is left pending: {values:?}");
                assert_eq!((values.file_count, values.dir_count), (Some(0), Some(0)));
            }
            other => panic!("expected InfoResolved, got {other:?}"),
        }
    }

    #[test]
    fn a_panicked_drive_label_lookup_resolves_to_no_label() {
        let (tx, rx) = mpsc::channel();
        spawn_drive_label_with(PanelSide::Left, 'E', 9, tx, WorkerGauge::new().token(), || panic!("scripted label panic"));
        match recv(&rx) {
            Command::DriveLabelResolved { target, letter, label, generation } => {
                assert_eq!((target, letter, label, generation), (PanelSide::Left, 'E', None, 9));
            }
            other => panic!("expected DriveLabelResolved, got {other:?}"),
        }
    }

    #[test]
    fn a_panicked_tree_expansion_resolves_to_no_children() {
        let (tx, rx) = mpsc::channel();
        spawn_tree_expand_with(PanelSide::Right, PathBuf::from("/tree"), tx, WorkerGauge::new().token(), |_| panic!("scripted tree panic"));
        match recv(&rx) {
            Command::TreeNodeExpanded { panel, path, children } => {
                assert_eq!((panel, path), (PanelSide::Right, PathBuf::from("/tree")));
                assert!(children.is_empty());
            }
            other => panic!("expected TreeNodeExpanded, got {other:?}"),
        }
    }

    #[test]
    fn a_panicked_find_walk_still_reports_the_search_done() {
        let (tx, rx) = mpsc::channel();
        spawn_find_subtree_with(PathBuf::from("/root"), "x".to_string(), 11, CancelFlag::new(), tx, WorkerGauge::new().token(), |_, _, _| {
            panic!("scripted find panic")
        });
        match recv(&rx) {
            Command::FindFileSearchDone { request } => assert_eq!(request, 11),
            other => panic!("expected FindFileSearchDone, got {other:?}"),
        }
    }

    #[test]
    fn a_panicked_viewer_search_reports_no_match() {
        let (tx, rx) = mpsc::channel();
        spawn_viewer_search_with(5, CancelFlag::new(), tx, WorkerGauge::new().token(), || panic!("scripted search panic"));
        match recv(&rx) {
            Command::ViewerSearchResult { offset: None, match_range: None, request: 5 } => {}
            other => panic!("expected a no-match ViewerSearchResult, got {other:?}"),
        }
    }

    #[test]
    fn a_panicked_worker_still_releases_its_gauge_token_after_replying() {
        let (tx, rx) = mpsc::channel();
        let gauge = WorkerGauge::new();
        spawn_drive_label_with(PanelSide::Left, 'E', 1, tx, gauge.token(), || panic!("boom"));
        let _ = recv(&rx);
        let deadline = Instant::now() + Duration::from_secs(5);
        while gauge.live() != 0 {
            assert!(Instant::now() < deadline, "the token is released once the reply is out");
            std::thread::yield_now();
        }
    }
}

pub fn spawn_job(job: Job, tx: Sender<Command>, token: WorkerToken) -> JobHandle {
    spawn_job_with(job, tx, token, |job, sink| {
        let fs = RealFs::new();
        filecommand_core::fs_ops::run_job(job, &fs, sink);
    })
}

/// `spawn_job` with the engine run injected (design D6): a panicked job
/// still ends — as `Cancelled` with one skipped item naming the internal
/// error against the source directory — so the progress or prompt dialog
/// closes, the panels re-read, and the end-of-job summary names the error
/// (application-shell "A panicked job ends and closes its dialog"). The
/// skipped list `run_job` had built up is lost with the unwound frame; the
/// sender is cloned before the sink takes it so the reply can still go out.
fn spawn_job_with<B>(job: Job, tx: Sender<Command>, token: WorkerToken, body: B) -> JobHandle
where
    B: FnOnce(&Job, &mut ChannelSink) + Send + 'static,
{
    let cancel = CancelFlag::new();
    let (reply_tx, reply_rx) = mpsc::channel::<JobReply>();
    let source_dir = job.source_dir.clone();
    let dest_dir = job.dest_dir.clone();
    let cancel_for_thread = cancel.clone();
    let thread = std::thread::spawn(move || {
        let _token = token;
        let failure_tx = tx.clone();
        let mut sink = ChannelSink {
            tx,
            reply_rx,
            cancel: cancel_for_thread,
            source_dir: source_dir.clone(),
            dest_dir: dest_dir.clone(),
            throttle: ProgressThrottle::new(),
        };
        if let Err(msg) = guarded(move || body(&job, &mut sink)) {
            log_worker_panic("job", &msg);
            let skipped = vec![SkippedItem::skipped(source_dir.clone(), format!("internal error: {msg}"))];
            deliver(&failure_tx, "job outcome", Command::JobDone { outcome: JobOutcome::Cancelled { skipped }, source_dir, dest_dir });
        }
    });
    JobHandle { cancel, reply_tx, thread }
}
