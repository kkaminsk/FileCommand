use super::*;
use crate::drives::DriveEntry;
use crate::listing::{Entry, EntryKind};
use crate::menu::MenuEntry;
use std::ffi::OsString;
// ---------------------------------------------------------------------
// M5: F4 built-in editor
// ---------------------------------------------------------------------

use crate::editor::{EditorMove, EditorState, EntryMode, ReplacePrompt};

mod editor;
mod file_op;
mod listing;
mod overlays;
mod panels;
mod routing;
mod viewer;

fn test_state(phase: UiPhase) -> State {
    // Both panels carry a listing request id, as `State::initial` would have
    // minted for their first listing (listing-request-guard design D1), so
    // the listing helpers below have an id to echo; `request_seq` sits past
    // them so the next minted id never collides.
    let mut left = PanelState::new(PathBuf::from("/left"));
    left.listing_request = Some(1);
    let mut right = PanelState::new(PathBuf::from("/right"));
    right.listing_request = Some(2);
    State { left, right, phase, request_seq: 2, ..State::empty(Theme::classic()) }
}

/// The id of `side`'s current listing — what a worker reply must echo to be
/// applied (listing-request-guard design D8).
fn listing_request(state: &State, side: PanelSide) -> u64 {
    state.panel(side).listing_request.expect("the panel has a current listing request")
}

/// Feed one `ListingChunk` for `side`'s *current* listing — the request id
/// the panel recorded when the listing began — so the test exercises the
/// accepted path exactly as the worker for that listing would (listing-
/// request-guard design D8). Tests about superseded replies construct their
/// mismatched `Command::ListingChunk { request, .. }` explicitly.
fn listing_chunk(state: State, side: PanelSide, entries: Vec<Entry>) -> (State, Vec<Effect>) {
    let request = listing_request(&state, side);
    update(state, Command::ListingChunk { panel: side, entries, request })
}

/// `ListingComplete` for `side`'s current listing; see [`listing_chunk`].
fn listing_complete(state: State, side: PanelSide, total: usize) -> (State, Vec<Effect>) {
    let request = listing_request(&state, side);
    update(state, Command::ListingComplete { panel: side, total, unreadable: 0, request })
}

/// `ListingFailed` for `side`'s current listing; see [`listing_chunk`].
fn listing_failed(state: State, side: PanelSide, message: String) -> (State, Vec<Effect>) {
    let request = listing_request(&state, side);
    update(state, Command::ListingFailed { panel: side, message, request })
}

/// The `Effect::StartListing` the reducer just issued for `side`: its
/// `request` is whatever `begin_listing_inner` recorded on the panel, read
/// back from the post-`update` state (listing-request-guard design D8).
fn start_listing(state: &State, side: PanelSide, path: impl Into<PathBuf>) -> Effect {
    Effect::StartListing { panel: side, path: path.into(), request: listing_request(state, side) }
}

fn file_entry(name: &str, size: u64) -> Entry {
    Entry::new(OsString::from(name), EntryKind::File, size, None, false, false)
}

fn dir_entry(name: &str) -> Entry {
    Entry::new(OsString::from(name), EntryKind::Directory, 0, None, false, false)
}

/// Every navigation/re-read now also issues `Effect::QueryGitInfo`
/// alongside `Effect::StartListing` (git-info "Query re-issued on
/// navigation"), `Effect::PersistHistory` (fuzzy-jump "Navigation
/// records history"; design D6), and — when the panel's previous listing
/// was still streaming, as `test_state`'s fresh panels always are —
/// `Effect::CancelRequest` for that listing (listing-request-guard design
/// D4). Tests that predate those changes and only care about the
/// listing-related effects filter all three out with this helper rather
/// than asserting on request ids/frecency bookkeeping that are incidental
/// to what they're testing; tests about cancellation read the raw effects.
fn without_git_info_effects(effects: Vec<Effect>) -> Vec<Effect> {
    effects.into_iter().filter(|e| !matches!(e, Effect::QueryGitInfo { .. } | Effect::PersistHistory(_) | Effect::CancelRequest(_))).collect()
}

/// Record an outstanding F3/F4 open on the active panel under `request`,
/// exactly as `handle_request_viewer`/`handle_request_editor` do, so a
/// reply a test injects directly is the current one and is applied rather
/// than discarded as stale (offload-blocking-io design D4). The id counter
/// is advanced past `request` so anything minted afterwards is fresh.
fn with_pending_open(mut state: State, request: u64) -> State {
    state.request_seq = state.request_seq.max(request);
    state.pending_open = Some(PendingOpen { request, side: state.active, kind: PendingOpenKind::Viewer });
    state
}

fn running_progress_state(kind: JobKind, source_dir: &str, dest_dir: &str) -> State {
    let mut state = test_state(UiPhase::FileOpRunning {
        source_dir: PathBuf::from(source_dir),
        dest_dir: PathBuf::from(dest_dir),
        dialog: RunningDialog::Progress { kind, progress: ProgressInfo::starting(3, 30) },
    });
    state.left.cwd = PathBuf::from(source_dir);
    state.right.cwd = PathBuf::from(dest_dir);
    state
}

fn sample_conflict() -> ConflictInfo {
    ConflictInfo {
        source_name: OsString::from("a.txt"),
        source_size: 1,
        source_modified: None,
        target_path: PathBuf::from("/right/a.txt"),
        target_size: 2,
        target_modified: None,
    }
}

// ---------------------------------------------------------------------
// M3: command line
// ---------------------------------------------------------------------

fn type_line(state: State, text: &str) -> State {
    text.chars().fold(state, |s, c| update(s, Command::TextEdit(TextEditOp::Insert(c))).0)
}

// ---------------------------------------------------------------------
// file-action-menu: navigation, dismissal, selection independence,
// precedence over a non-empty command buffer, and per-action routing.
// ---------------------------------------------------------------------

/// Cursor on `cursor_on` among `names`, menu already open.
fn opened_menu_state(names: &[&str], cursor_on: &str) -> State {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = names.iter().map(|n| file_entry(n, 1)).collect();
    state.left.cursor = names.iter().position(|n| *n == cursor_on).unwrap();
    let (state, _) = update(state, Command::Enter);
    assert!(state.file_action_menu().is_some(), "setup precondition: the menu must be open");
    state
}

// ---------------------------------------------------------------------
// pulldown-menu-completion: Swap panels (task 2.5; panel-navigation "Swap
// panels (Ctrl+U)"; design D3)
// ---------------------------------------------------------------------

/// Both panels' listings marked Complete, the state every swap test starts
/// from (a Streaming panel refuses the swap).
fn complete_panel_listings(state: &mut State) {
    state.left.progress = crate::panel::ListingProgress::Complete { count: state.left.entries.len() };
    state.right.progress = crate::panel::ListingProgress::Complete { count: state.right.entries.len() };
}

fn opened_viewer_state(file_len: u64) -> State {
    let state = with_pending_open(test_state(UiPhase::Panels), 3);
    let (state, _) = update(state, Command::ViewerOpened { request: 3, path: PathBuf::from("/left/f.txt"), file_len });
    state
}

/// Run a real F7 search-confirm sequence so the returned state carries the
/// actual outstanding `search_request` id, mirroring how the TUI event loop
/// would drive it (`ViewerSearchStart` -> typed chars -> `ViewerSearchConfirm`).
fn viewer_with_pattern_confirmed(state: State, pattern: &str) -> (State, u64) {
    let (state, _) = update(state, Command::ViewerSearchStart);
    let mut state = state;
    for c in pattern.chars() {
        let (s, _) = update(state, Command::ViewerSearchChar(c));
        state = s;
    }
    let (state, _) = update(state, Command::ViewerSearchConfirm);
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    let request = v.search_request.expect("search confirm must record an outstanding request id");
    (state, request)
}

fn editor_with(lines: &[&str]) -> EditorState {
    let text = lines.join("\n");
    let mut bytes = text.into_bytes();
    if !lines.is_empty() {
        bytes.push(b'\n');
    }
    EditorState::from_bytes(PathBuf::from("/left/edit.txt"), &bytes).expect("test fixture is valid UTF-8")
}

/// A real on-disk file to back tests that exercise an actual
/// `EditorState::save` round trip (the `Effect::SaveEditor` reply tests),
/// rather than a bare in-memory `from_bytes` buffer whose path doesn't
/// exist.
fn editor_with_temp_file(name: &str, lines: &[&str]) -> EditorState {
    let dir = std::env::temp_dir().join(format!("filecommand-update-test-editor-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("file.txt");
    let mut text = lines.join("\n");
    if !lines.is_empty() {
        text.push('\n');
    }
    std::fs::write(&path, &text).unwrap();
    match EditorState::open(&path).unwrap() {
        crate::editor::LoadResult::Loaded(e) => *e,
        crate::editor::LoadResult::TooLarge { .. } => panic!("test fixture unexpectedly exceeds the editor size cap"),
        crate::editor::LoadResult::NotEditable { reason } => panic!("test fixture unexpectedly not editable: {reason:?}"),
    }
}

fn editor_state(editor: EditorState) -> State {
    State { phase: UiPhase::Editor(editor), ..test_state(UiPhase::Panels) }
}

// ---------------------------------------------------------------------
// M5: Tree display mode (design D7; additional-panel-modes)
// ---------------------------------------------------------------------

fn dir_child(name: &str) -> Entry {
    dir_entry(name)
}

// ---------------------------------------------------------------------
// M5: Ctrl+J fuzzy jump
// ---------------------------------------------------------------------

fn history_entry(path: &str, count: u32, ms: u64) -> crate::quicksearch::FrecencyEntry {
    crate::quicksearch::FrecencyEntry { path: PathBuf::from(path), visit_count: count, last_visited_ms: ms }
}
