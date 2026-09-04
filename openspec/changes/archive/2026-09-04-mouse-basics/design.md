# Design: mouse-basics

## Context

The event loop (`crates/filecommand-tui/src/app.rs`) reads one crossterm event per iteration and handles `Event::Key` and `Event::Resize` only. `views::render` already returns a value (the editor caret position), so returning a hit map alongside is a natural extension. `TerminalGuard` (`terminal.rs`) centralises entering/leaving the alternate screen and raw mode, with `suspend()`/`resume()` for shell runs and `restore_terminal()` for `Drop` and the panic hook. crossterm delivers `MouseEventKind::{Down, Up, Drag, Moved, ScrollUp, ScrollDown}` with modifier flags on every event, on both the Windows console backend and SGR terminals; Windows Terminal, iTerm2, Kitty, and GNOME Terminal reserve Shift+click/drag for native selection, and enabling capture on conhost disables QuickEdit for the session. The `application-shell` spec mandates a pure core `update` — raw coordinates must not enter core. The `file-action-menu` (from the unarchived `enter-file-action-menu` change) is file-only and single-target by spec.

## Goals / Non-Goals

**Goals:**

- Every existing action a mouse user would expect is reachable by click; the mouse never becomes the only route to anything.
- Capture is never leaked to a child shell or left on after exit.
- Core stays pure and unit-testable.
- No visual change beyond one added Help topic — every other snapshot golden stays byte-identical.

**Non-Goals:**

- Drag and drop (`mouse-panel-drag`).
- Hover highlighting; touch.
- Column-header sorting and editor click-to-caret (later).
- Mouse in overlays not listed in the gating table (drive select, fuzzy jump, find file, user menu, theme picker, help) — v2.

## Decisions

### D1: Capture is on by default and owned by `TerminalGuard`

Per the user's decision. `TerminalGuard::new` issues `EnableMouseCapture` when configured; `suspend()` issues `DisableMouseCapture` before `LeaveAlternateScreen` (otherwise a child shell inherits mouse tracking, and on Windows keeps QuickEdit off); `resume()` re-enables capture, and the three `guard.resume()` call sites in `app.rs` (shell command, external editor, scrollback) reset the TUI-side `MouseTracker`; `restore_terminal()` issues `DisableMouseCapture` unconditionally (harmless when never enabled; the panic hook has no access to the flag). Off switch: `[mouse] enabled = false` or `--nomouse` (mirroring `--nosplash`). Rejected: a menu toggle — it would need a pull-down delta and runtime state for a rarely-changed setting; a config key plus a launch flag suffice.

### D2: Hit map in the TUI, semantic commands into core

`views::render` returns `HitMap { panels: [PanelHits; 2], keybar: Vec<(Rect, u8)>, menu_titles, menu_items, dialog_buttons, cmdline }` where `PanelHits { area, title, rows: Vec<(Rect, OsString)> }` keys rows by the entry's original name — the same identity that keys the selection set — never by index. `input::map_mouse(MouseEvent, &HitMap, &mut MouseTracker, &State) -> Option<Command>` sits beside `map_key`. Core gains `ClickEntry { side, name, mods: ClickMods }`, `FocusPanel(side)`, `ScrollPanel { side, delta }`, `KeybarPress(u8)`, `MenuTitleClick(MenuId)`, `MenuItemClick(usize)`, `DialogButtonClick(ButtonId)`, and `OpenActionMenuAt { side, name }`. `ClickMods` is a core enum (`Plain | Ctrl | Shift`) so `crossterm::KeyModifiers` never crosses the boundary. "Dialog buttons" includes the hotkey text spans of dialogs that have no framed buttons (the conflict dialog's `(O)verwrite  (S)kip …` row): the hit map records each span as a button. Rejected: coordinates in core — violates the pure-update requirement and turns every layout change into a core change.

### D3: Click semantics follow Norton Commander, not Explorer

A click on a row focuses the panel and moves the cursor; it never toggles selection (NC selection is sticky — click-to-deselect would fight `Ins` users). Ctrl+Down then Up with no movement toggles the row, like `Ins` without advancing. Double-click (a second `Down` on the same row within ~400 ms) = Enter. Shift+click range selection is intercepted by most emulators and is deliberately not specified.

### D4: Right-click opens the action menu, scoped to what was clicked

Files: the existing Enter menu with the cursor moved first. Directories: the same menu without View, Edit, and Run, which have no meaning for a directory. A right-click on a *selected* entry scopes Copy/Move/Delete/Send to clipboard to the selection set and titles the dialog with the count. This is an ADDED `file-action-menu` requirement rather than a modification of "Menu contents, ordering, and navigation" (which `clipboard-file-export` modifies), so the two proposals cannot clobber each other regardless of archive order.

### D5: Mode gating mirrors the keyboard

Mouse honoured: the panels phase (no quit dialog / startup warning); an open pull-down (title/item clicks; a click elsewhere closes it); the file-op setup and summary dialogs, conflict, error, delete-confirm, and quit-confirm dialogs (buttons only); the viewer and editor (wheel only). Everything else ignores mouse events. A running progress dialog accepts a click on Cancel only.

### D6: Wheel semantics respect the cursor-in-window invariant

`panel-scrolling` requires the cursor to stay inside the viewport and re-clamps the offset after every mutation, so a wheel that moved the viewport and left the cursor behind would violate it. The wheel therefore moves the *cursor* of the panel under the pointer by three rows (`ScrollPanel { side, delta }` → the existing cursor-move path), and the viewport follows through the existing clamp; the active panel is not changed. In the viewer a notch is `ViewerInput::ScrollLines(±3)` through `resolve_viewer_navigation` → `Command::ViewerSetTop`. The built-in editor has no caret-independent viewport scroll (`top_line` only follows the caret), so a notch is three `EditorMove::Down`/`Up` steps. Rejected: a MODIFIED `panel-navigation` delta relaxing the invariant for wheel scrolling — more spec surface for a marginal gain.

### D7: Coalescing

After the first event each frame the loop drains `event::poll(Duration::ZERO)`; `Moved` events are discarded (no hover feature) and consecutive wheel notches are summed, so pointer motion never drives a redraw storm.

## Risks / Trade-offs

- [Users lose plain-drag text selection in the terminal] → Shift+drag still selects natively; `[mouse] enabled = false` / `--nomouse`; documented in the Help `Mouse` topic.
- [Capture leaked to a child shell on one suspend path] → single seam in `TerminalGuard`; an integration test covers suspend/resume ordering.
- [crossterm believes a button is still held after resume] → `MouseTracker` is reset on `resume()`.
- [conhost QuickEdit is disabled while running] → inherent to capture; restored on exit and during suspend.

## Open Questions

- None blocking.
