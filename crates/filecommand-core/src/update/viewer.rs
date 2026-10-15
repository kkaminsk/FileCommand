//! The F3 viewer: opening it on a worker thread and driving it while it owns
//! input.

use super::*;

// ---------------------------------------------------------------------
// F3 viewer (M4)
// ---------------------------------------------------------------------

/// F3: open the viewer on the file under the active panel's cursor. A no-op
/// on a directory (including `..`) or an empty panel; opening itself is an
/// `Effect` because it touches the filesystem, which `update` never does
/// directly (viewer: Instant open).
pub(super) fn handle_request_viewer(state: &mut State) -> Vec<Effect> {
    let side = state.active;
    let Some(entry) = state.panel(side).selected() else { return vec![] };
    if entry.is_dir_like() {
        return vec![];
    }
    let path = state.panel(side).cwd.join(&entry.name);
    vec![request_open(state, side, PendingOpenKind::Viewer, path)]
}

/// Record an outstanding F3/F4 open for `side` under a fresh request id and
/// return the effect that runs it on a worker thread (offload-blocking-io
/// design D4). The panel announces it as `Opening <name>…` on its
/// mini-status line, which also clears a stale `last_error` from an earlier
/// failed attempt (design D3). A newer request replaces the pending entry,
/// which is what supersedes the older open: its reply no longer matches
/// (application-shell "A newer F3 supersedes an outstanding open").
pub(super) fn request_open(state: &mut State, side: PanelSide, kind: PendingOpenKind, path: PathBuf) -> Effect {
    let request = state.next_request_id();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
    state.panel_mut(side).set_pending_note(format!("Opening {name}…"));
    state.pending_open = Some(PendingOpen { request, side, kind });
    match kind {
        PendingOpenKind::Viewer => Effect::OpenViewer { request, path },
        PendingOpenKind::Editor => Effect::OpenEditor { request, path },
    }
}

/// Match an F3/F4 open reply against the pending open (design D4). Returns
/// the panel the open was requested from when the reply is current and the
/// panels still own input — the caller then applies it. A reply whose id
/// is not the pending one belongs to a superseded request and is dropped
/// without touching the newer, still-outstanding one. A reply whose id
/// matches but arrives after the user left the panels or opened a menu,
/// dialog or overlay clears the pending state and is discarded
/// (application-shell "A late open does not pull the user out of a menu").
/// Either way the panel's `Opening…` note comes down once its reply is in.
pub(super) fn take_current_open(state: &mut State, request: u64) -> Option<PanelSide> {
    if state.pending_open.as_ref().map(|p| p.request) != Some(request) {
        return None;
    }
    let pending = state.pending_open.take()?;
    state.panel_mut(pending.side).pending_note = None;
    panels_own_input(state).then_some(pending.side)
}

/// Drive every command while the viewer is open (gated in [`update`] by
/// `UiPhase::Viewer`). Byte-level work (backward scan, search) is never
/// performed here — it requires file I/O that `update` must stay free of —
/// so `ViewerSetTop` carries an already-computed offset and
/// `ViewerSearchConfirm` only kicks off `Effect::RunViewerSearch`, whose
/// result later re-enters as `Command::ViewerSearchResult` (design D1).
pub(super) fn handle_viewer(state: &mut State, cmd: Command) -> Vec<Effect> {
    let mut effects = Vec::new();
    let UiPhase::Viewer(mut viewer) = std::mem::replace(&mut state.phase, UiPhase::Panels) else {
        unreachable!("handle_viewer only called when phase is Viewer");
    };
    match cmd {
        Command::ViewerClose => {
            // A search still scanning is stopped with the viewer, releasing
            // the worker's own handle on the file (viewer "Search stops when
            // superseded or when the viewer closes"; listing-request-guard
            // design D4). The id stays `Some` after a result arrives, but
            // cancelling an id with no running worker is a no-op (D3).
            effects.extend(viewer.search_request.map(Effect::CancelRequest));
            return effects;
        }
        Command::ViewerToggleMode => viewer.toggle_mode(),
        Command::ViewerToggleWrap => viewer.toggle_wrap(),
        Command::ViewerSetTop(offset) => viewer.set_top_offset(offset),
        Command::ViewerSetHScroll(col) => viewer.h_scroll = col,
        Command::ViewerSearchStart => viewer.search_input = Some(String::new()),
        Command::ViewerSearchChar(c) => {
            if let Some(input) = &mut viewer.search_input {
                input.push(c);
            }
        }
        Command::ViewerSearchBackspace => {
            if let Some(input) = &mut viewer.search_input {
                input.pop();
            }
        }
        Command::ViewerSearchCancel => viewer.search_input = None,
        Command::ViewerSearchConfirm => {
            if let Some(pattern) = viewer.search_input.take().filter(|p| !p.is_empty()) {
                let pattern_bytes = pattern.into_bytes();
                viewer.search_pattern = Some(pattern_bytes.clone());
                let path = viewer.path.clone();
                let start_offset = viewer.top_offset;
                // A fresh request id per search, mirroring
                // `PanelState::info_request` — lets a reply be matched
                // against the search that's still outstanding, so an
                // out-of-order or superseded-session reply is dropped
                // rather than applied (viewer: F7 streaming search
                // staleness). The previous search, if still scanning, is
                // stopped rather than left to finish a multi-GB scan whose
                // result would be dropped anyway (viewer "A newer search
                // supersedes a running one"; listing-request-guard design
                // D4).
                effects.extend(viewer.search_request.map(Effect::CancelRequest));
                let request = state.next_request_id();
                viewer.search_request = Some(request);
                state.phase = UiPhase::Viewer(viewer);
                effects.push(Effect::RunViewerSearch { path, start_offset, pattern: pattern_bytes, request });
                return effects;
            }
        }
        // Only apply a reply whose id matches this session's outstanding
        // search. A mismatch means either a stale reply from a search this
        // same session has since superseded, or one from a viewer session
        // that has since closed and been reopened (a fresh `ViewerState`
        // starts with `search_request: None`, which can never equal a real
        // request id) — either way it is silently dropped instead of
        // jumping the user to a bogus offset with a phantom match
        // highlight.
        Command::ViewerSearchResult { offset: Some(offset), match_range, request } if viewer.search_request == Some(request) => {
            viewer.set_top_offset(offset);
            viewer.last_match = match_range;
        }
        _ => {}
    }
    state.phase = UiPhase::Viewer(viewer);
    effects
}
