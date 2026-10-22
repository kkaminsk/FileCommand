//! Directory listings: starting one, folding its streamed replies into the
//! panel, git-info queries, and the viewport reconciliation every cursor- or
//! list-mutating path funnels through.

use super::*;

/// A panel's body entry-row count for a given terminal height, display mode,
/// and tab count — the same shape as `editor_viewport`, kept as a
/// duplicate of `filecommand-tui`'s `layout::compute` +
/// `views::panel::render` geometry rather than a cross-crate dependency, so
/// `core::update` can call [`PanelState::ensure_cursor_visible`] without
/// depending on `filecommand-tui` (design D2). Only the terminal height
/// matters here — the command-line and F-key bar rows are shared by both
/// panels regardless of the panel split, and `split_percent` only ever
/// changes each panel's *width*, which entry-row counting doesn't depend on
/// (Brief mode's column count, a width-derived quantity, is the
/// `additional-panel-modes` follow-up group's concern, not this one's).
///
/// Mirrors, in order: `layout::compute`'s `panels_h` (terminal height minus
/// the one-row command line and one-row F-key bar), `views::panel::render`'s
/// `has_strip`/`reserved` (a tab strip, shown at 2+ tabs, costs a row beyond
/// the top+bottom border), and its `rows_h` per display mode — Full and Tree
/// both reserve a header row on top of the body, Brief does not.
///
/// `pub` so `filecommand-tui` can also derive the PgUp/PgDn paging step from
/// it directly, in place of the layout-level `entries_visible` (which
/// over-counts by one when the tab strip is visible and mismatches Brief) —
/// one source of truth for both the viewport clamp and the paging step
/// (design D2's risk note).
pub fn panel_viewport_rows(term_size: (u16, u16), display_mode: DisplayMode, tab_count: usize) -> usize {
    let panels_h = term_size.1.saturating_sub(2);
    let has_strip = panels_h >= 4 && tab_count >= 2;
    let reserved: u16 = if has_strip { 3 } else { 2 };
    let body_h = panels_h.saturating_sub(reserved);
    let rows = match display_mode {
        DisplayMode::Full | DisplayMode::Tree => body_h.saturating_sub(1),
        _ => body_h,
    };
    rows.max(1) as usize
}

/// `side`'s panel interior width (the panel's rendered width, border
/// columns subtracted) for the current terminal width and split — mirrors
/// `layout::compute`'s `left_w`/`right_w` (via
/// `panel_split::effective_left_width`) and `views::panel::render`'s
/// `w.saturating_sub(2)`, kept as a duplicate here for the same reason
/// [`panel_viewport_rows`] is: `core::update` needs it (for Brief mode's
/// column count) without a cross-crate dependency on `filecommand-tui`.
pub(super) fn panel_interior_width(term_size: (u16, u16), split_percent: u16, side: PanelSide) -> u16 {
    let left_w = panel_split::effective_left_width(split_percent, term_size.0);
    let panel_w = match side {
        PanelSide::Left => left_w,
        PanelSide::Right => term_size.0.saturating_sub(left_w),
    };
    panel_w.saturating_sub(2)
}

/// Brief mode's column count for a given interior width: `max(1,
/// floor(interior_w / 12))`, byte-identical to `filecommand-tui`'s
/// `brief_column_widths`/`render_brief_body` formula (design D4;
/// additional-panel-modes "Brief mode column scrolling").
pub(super) fn brief_column_count(interior_w: u16) -> usize {
    ((interior_w / 12).max(1)) as usize
}

/// Re-clamp `side`'s scroll offset against its current viewport — the one
/// call site every cursor-moving or list-mutating path funnels through
/// (design D6), so the renderer never has to reason about scrolling itself.
/// Dispatches to the mode-appropriate clamp: Brief's column-window
/// reconciliation, Tree's over its own `TreeState::scroll_offset` (a no-op
/// if Tree mode was already left before this ran), or Full/Info/QuickView's
/// line-wise `PanelState::ensure_cursor_visible`.
pub(super) fn reconcile_panel_viewport(state: &mut State, side: PanelSide) {
    let panel = state.panel(side);
    let display_mode = panel.display_mode;
    let rows = panel_viewport_rows(state.term_size, display_mode, panel.tab_count());
    match display_mode {
        DisplayMode::Brief => {
            let interior_w = panel_interior_width(state.term_size, state.split_percent, side);
            let cols = brief_column_count(interior_w);
            state.panel_mut(side).ensure_cursor_visible_brief(rows, cols);
        }
        DisplayMode::Tree => {
            if let Some(tree) = state.panel_mut(side).tree.as_mut() {
                tree.ensure_cursor_visible(rows);
            }
        }
        _ => {
            state.panel_mut(side).ensure_cursor_visible(rows);
        }
    }
}

/// The choke point every deliberate navigation of `side`'s directory flows
/// through — Enter into a directory, `..`, a typed `cd`, drive select,
/// Tree's own Enter-to-return, and the M5 fuzzy-jump/find-file dialogs — so
/// it is also where the Ctrl+J frecency history is recorded and persisted
/// (fuzzy-jump "Navigation records history"; design D6). Re-reads that are
/// not "the user navigating the active panel into a directory" bypass this
/// via [`begin_listing_inner`] directly — see its doc comment for the two
/// non-navigation callers.
pub(super) fn begin_listing(state: &mut State, side: PanelSide, path: PathBuf) -> Vec<Effect> {
    // Visits are stamped with wall-clock time so recency survives a restart
    // (fuzzy-jump "Frecency ranking"; history-persistence-hardening D1).
    quicksearch::record_visit(&mut state.dir_history, &path, state.epoch_ms);
    let mut effects = begin_listing_inner(state, side, path, true);
    effects.push(Effect::PersistHistory(config::HistoryFile { commands: state.history.clone(), directories: state.dir_history.clone() }));
    effects
}

/// Everything [`begin_listing`] does except the navigation bookkeeping (the
/// frecency visit and the history persist) — the entry point for re-reads
/// that are not "the user navigating the active panel into a directory".
/// Two callers qualify today: Tree mode's cursor-move preview of the
/// *opposite* panel (that is the tree being browsed, not a navigation), and
/// the stale-tab refresh on `SwitchTab`/`CloseTab` activation (the user
/// changed tabs, not directories — panel-tabs "Stale background tab refresh
/// on activation"). A third caller should prompt extracting a purpose-named
/// helper rather than diluting this contract.
///
/// `capture_anchor` asks for the panel's cursor entry and scroll offset to
/// be snapshotted before the listing resets — which happens only when
/// `path` is the directory the panel is already showing, so the streamed
/// re-listing can restore the user's place (panel-navigation "Same-
/// directory re-read preserves cursor and scroll position"; reread-
/// preserves-cursor design D5). Every caller passes `true` except the
/// Tree-mode preview, whose opposite-panel re-listing is a navigation of
/// that panel, not a re-read the user asked for.
pub(super) fn begin_listing_inner(state: &mut State, side: PanelSide, path: PathBuf, capture_anchor: bool) -> Vec<Effect> {
    let mut effects = Vec::new();
    // A listing still being read is superseded by this one: its reader is
    // told to stop, and its remaining replies — tagged with the id it was
    // issued under — will fail the `listing_request` check below (panel-
    // navigation "Superseded directory listings are discarded and
    // cancelled"; listing-request-guard design D4). A completed listing has
    // no worker left to stop.
    if state.panel(side).progress.is_streaming() {
        if let Some(previous) = state.panel(side).listing_request {
            effects.push(Effect::CancelRequest(previous));
        }
    }
    if capture_anchor && path == state.panel(side).cwd {
        state.panel_mut(side).capture_reread_anchor();
    }
    state.panel_mut(side).begin_new_listing(path.clone());
    // A fresh id per listing, minted even for a re-read of the same path, so
    // two quick Ctrl+Rs are two distinguishable generations (design D1).
    let request = state.next_request_id();
    state.panel_mut(side).listing_request = Some(request);
    effects.push(Effect::StartListing { panel: side, path: path.clone(), request });
    // A panel sitting in Info mode needs its drive/directory figures
    // re-gathered for wherever it just landed. A fresh request id is
    // minted even when the path is unchanged (e.g. a re-read), so an
    // answer to a since-superseded query for the same path is still
    // recognized as stale and dropped.
    if state.panel(side).display_mode == DisplayMode::Info {
        let request = state.next_request_id();
        state.panel_mut(side).info_request = Some(request);
        effects.push(Effect::QueryInfo { panel: side, path: path.clone(), request });
    }
    // Every navigation (and re-read) re-issues the git-info query for
    // wherever the panel just landed, regardless of display mode — the
    // branch suffix and marker column are independent of Info mode (git-info
    // "Query re-issued on navigation") — unless the path is under a
    // repository already marked "no info" this session.
    effects.extend(git_info_query_effect(state, side, path));
    effects
}

/// The `CancelRequest` for `side`'s active tab's listing when that listing
/// is still being read — what `SwitchTab`/`CloseTab` emit once the tab has
/// actually stopped being active (panel-tabs "Switching or closing a tab
/// mid-listing never mixes listings"; listing-request-guard design D4).
/// Empty when the active tab's listing is complete.
pub(super) fn leaving_tab_listing(state: &State, side: PanelSide) -> Option<Effect> {
    let p = state.panel(side);
    if p.progress.is_streaming() {
        p.listing_request.map(Effect::CancelRequest)
    } else {
        None
    }
}

/// The fresh read a stale tab gets the moment it becomes active, on
/// `SwitchTab`/`CloseTab`. Not a navigation (the user changed tabs, not
/// directories), so it goes through `begin_listing_inner`: a fresh read
/// without the frecency visit or history persist (panel-tabs "Stale
/// background tab refresh on activation"). It *is* a same-directory re-read
/// of the restored tab, so the anchor capture stays on — except when the
/// tab was left mid-listing: its progress is still `Streaming`, and
/// `capture_reread_anchor` would *clear* the anchor an interrupted re-read
/// stashed with the tab (it only anchors to a `Complete` listing). Skipping
/// the capture lets `begin_new_listing` keep that anchor for the same `cwd`,
/// so the refresh restores the place the interrupted re-read was restoring;
/// a tab interrupted mid-navigation has no anchor and starts from the top
/// (panel-tabs "An interrupted re-read still keeps the user's place";
/// listing-request-guard design D6).
pub(super) fn refresh_activated_stale_tab(state: &mut State, side: PanelSide) -> Vec<Effect> {
    let path = state.panel(side).cwd.clone();
    let capture_anchor = !state.panel(side).progress.is_streaming();
    begin_listing_inner(state, side, path, capture_anchor)
}

/// Mint a fresh generation id, record it as `side`'s outstanding git-info
/// request, and build the effect that runs the query on a worker thread
/// (git-info "Background repository detection"; design D3) — or, for a path
/// under a repository a timed-out query marked "no info" this session,
/// `None`: no query is issued there, and the panel's git info is explicitly
/// reset so a panel swapped in cannot keep showing branch/markers it
/// resolved before the timeout (git-info "Timed-out repository is not
/// queried again", "Swapping a panel into a timed-out repository";
/// git-info-timeout-memory design D1).
pub(super) fn git_info_query_effect(state: &mut State, side: PanelSide, path: PathBuf) -> Option<Effect> {
    if state.git_no_info_roots.iter().any(|root| git_info::is_under_root(&path, root)) {
        let p = state.panel_mut(side);
        p.git_info = GitInfo::none();
        p.git_request = None;
        return None;
    }
    let request = state.next_request_id();
    state.panel_mut(side).git_request = Some(request);
    Some(Effect::QueryGitInfo { panel: side, path, request })
}

/// Whether a listing reply belongs to `panel`'s current listing — the one
/// whose id `begin_listing_inner` (or `State::initial`) recorded in
/// `listing_request`. A completed listing keeps its id, so a late reply
/// from an earlier generation still mismatches (listing-request-guard
/// design D2).
pub(super) fn listing_reply_is_current(state: &State, cmd: &Command) -> bool {
    match cmd {
        Command::ListingChunk { panel, request, .. }
        | Command::ListingComplete { panel, request, .. }
        | Command::ListingFailed { panel, request, .. } => state.panel(*panel).listing_request == Some(*request),
        _ => false,
    }
}

/// The mini-status line for a listing that skipped `n > 0` entries whose
/// metadata could not be read: `1 entry could not be read`, or `N entries
/// could not be read` with N comma-grouped (link-aware-listing design D7).
pub(super) fn unreadable_message(n: usize) -> String {
    if n == 1 {
        "1 entry could not be read".to_string()
    } else {
        format!("{} entries could not be read", format_count(n))
    }
}

pub(super) fn apply_listing_event(state: &mut State, cmd: Command) {
    // A reply from a superseded listing is dropped before any mutation —
    // entries, progress count, re-read anchor, find-file cursor target,
    // selection reconcile — so a directory the user has already left never
    // adds to, completes, or fails the listing that replaced it (panel-
    // navigation "Superseded directory listings are discarded and
    // cancelled"; listing-request-guard design D2).
    if !listing_reply_is_current(state, &cmd) {
        return;
    }
    match cmd {
        Command::ListingChunk { panel, entries, .. } => {
            let term_size = state.term_size;
            let p = state.panel_mut(panel);
            // One sort-and-merge per batch, never a per-entry insertion
            // (panel-navigation "Large directories stream progressively and
            // stay responsive"; streaming-listing-performance design D3).
            p.merge_streamed(entries);
            if let crate::panel::ListingProgress::Streaming { count } = &mut p.progress {
                *count = p.entries.len();
            }
            // A same-directory re-read re-anchors the cursor and offset as
            // soon as a chunk containing the anchored entry lands, so the
            // restored position holds while the listing streams instead of
            // sitting on row 0 until completion (panel-navigation "No jump
            // while the re-read streams"; reread-preserves-cursor design
            // D2). A no-op when no anchor was captured.
            let rows = panel_viewport_rows(term_size, p.display_mode, p.tab_count());
            p.apply_reread_anchor(rows);
            // Keeps the pinned-to-top offset (0) while the user hasn't moved
            // the cursor, exactly as `merge_streamed` already pins the
            // cursor itself; re-clamps if the user had already moved it
            // (panel-navigation "Streamed listing keeps the top pinned until
            // the user moves"). In Brief mode this same call routes the
            // restored offset through `ensure_cursor_visible_brief`, keeping
            // it on a whole-column multiple (reread-preserves-cursor design
            // D3).
            reconcile_panel_viewport(state, panel);
        }
        Command::ListingComplete { panel, total, unreadable, .. } => {
            let term_size = state.term_size;
            let p = state.panel_mut(panel);
            p.progress = crate::panel::ListingProgress::Complete { count: total };
            p.release_merge_scratch();
            // Entries the reader skipped are reported through the existing
            // inline-message surface, in place of the cursored entry's
            // details the mini-status otherwise restores on completion;
            // `begin_new_listing` clears it on the panel's next listing
            // (panel-navigation "One unreadable entry does not fail a
            // listing", "Streaming listing mini-status"; link-aware-listing
            // design D7).
            if unreadable > 0 {
                p.last_error = Some(unreadable_message(unreadable));
            }
            p.clamp_cursor();
            p.reconcile_selection();
            // A find-file navigation seeds this with the matched entry's
            // name (`update::handle_find_file`'s `FindFileConfirm`); once
            // this directory's listing has actually landed, settle the
            // cursor on it (find-file "Navigate to a chosen result" —
            // "cursor positioned on the matched entry").
            let mut settled_on_target = false;
            if let Some(name) = p.pending_cursor_target.take() {
                if let Some(idx) = p.entries.iter().position(|e| e.name == name) {
                    p.cursor = idx;
                    p.cursor_user_moved = true;
                    settled_on_target = true;
                }
            }
            // Find-file's deferred settle keeps precedence when both are set
            // on the same listing (reread-preserves-cursor design D2);
            // either way the anchor is consumed here — until now chunk-time
            // applications re-resolved against it.
            if !settled_on_target {
                let rows = panel_viewport_rows(term_size, p.display_mode, p.tab_count());
                p.apply_reread_anchor(rows);
            }
            p.reread_anchor = None;
            reconcile_panel_viewport(state, panel);
        }
        Command::ListingFailed { panel, message, .. } => {
            let p = state.panel_mut(panel);
            p.progress = crate::panel::ListingProgress::Complete { count: p.entries.len() };
            p.release_merge_scratch();
            p.last_error = Some(message);
        }
        _ => unreachable!("apply_listing_event only called for listing events"),
    }
}
