# Change: mouse-basics

## Why

FileCommand is keyboard-driven and has no mouse handling at all — `EnableMouseCapture` is never issued and `Event::Mouse` is never read. Users who reach for the mouse get nothing: no click to focus a panel, no wheel, no clicking the function-key bar or a menu, and a dialog's buttons are decoration. Norton Commander 5 itself was mouse-aware. This change adds the mouse as a second door to actions that already exist, without taking anything from keyboard users, and lays the hit-testing groundwork that panel-to-panel drag (`mouse-panel-drag`) builds on.

## What Changes

- **Mouse capture on by default**, owned by the terminal guard: enabled at startup, released before any suspended-TUI shell run, re-enabled on resume, and released unconditionally on every exit path including the panic hook. `config.toml` `[mouse] enabled = false` or the `--nomouse` launch flag turns it off. Shift+drag remains the emulator's native text selection.
- **Clicks**: left-click focuses a panel and places the cursor (never deselects); double-click acts as Enter; clicking a function-key-bar slot, a menu-bar title, a pull-down item, or a dialog button dispatches exactly what the key would; clicking outside an open pull-down closes it; Ctrl+click toggles an entry's selection.
- **Wheel** moves the cursor of the panel under the pointer by three rows per notch (the viewport follows through the existing scroll-offset rules, so the cursor-in-window invariant holds and the active panel does not change); in the viewer it scrolls three lines via the existing `ScrollLines` path, in the built-in editor it moves the caret three lines.
- **Right-click** opens the file-action menu for the clicked entry: for a directory the menu omits View, Edit, and Run; on a selected entry Copy/Move/Delete/Send to clipboard act on the selection.
- **Mode gating**: the mouse is honoured only where the equivalent key would be; overlays that are not listed ignore it.
- **Architecture**: each render records a `HitMap` (panel rects, per-row entry names, key-bar slots, menu titles/items, dialog buttons) in the TUI; `input::map_mouse` turns raw events into semantic commands (`ClickEntry`, `FocusPanel`, `ScrollPanel`, `KeybarPress`, `MenuTitleClick`, `MenuItemClick`, `DialogButtonClick`, `OpenActionMenuAt`). Raw coordinates never reach `filecommand-core`. Mouse events are coalesced per frame; pointer motion without a button is discarded.
- **Help**: new `Mouse` topic.

## Capabilities

### New Capabilities

- `mouse-input`: capture lifecycle and configuration; click/double-click/wheel/right-click/Ctrl+click semantics; clickable key bar, menus, and dialog buttons; mode gating; hit-map architecture; event coalescing.

### Modified Capabilities

- `application-shell`: "Terminal ownership and restoration on every exit" and "Panic hook restores the terminal before reporting" — mouse capture becomes part of the acquired/released terminal state, including across suspend/resume.
- `file-action-menu`: new requirement — directory targets and selection-scoped invocation (used by right-click).
- `help-and-about`: "Help topic list" — `Mouse` topic added.

## Impact

- `crates/filecommand-tui/src/terminal.rs` — `TerminalGuard` gains mouse capture in `new`/`suspend`/`resume`; `restore_terminal()` releases it unconditionally; `tests/panic_restoration.rs` extended.
- `crates/filecommand-tui/src/hitmap.rs` (new); `views/mod.rs`, `views/panel.rs`, `views/keybar.rs`, `views/menubar.rs`, and the dialog views record rects while rendering.
- `crates/filecommand-tui/src/input/mod.rs` — `map_mouse` and `MouseTracker` (press bookkeeping, double-click timing).
- `crates/filecommand-tui/src/app.rs` — event-loop drain/coalesce, `Event::Mouse` routing, `--nomouse`.
- `crates/filecommand-core/src/update.rs` — semantic commands and their reducer arms; right-click action-menu scoping. `config.rs` — `[mouse]` table. `dialogs.rs` — `HELP_TOPICS` grows to 11 entries (inserting `Mouse` shifts every later topic index; `topic_page_text` arms and the index-based help tests renumber) plus the `Mouse` page text.
- Snapshot goldens: byte-identical except the Help topic-list golden, which gains one row.
- Depends on the `enter-file-action-menu` change (implemented, not yet archived) for the action menu, on `panel-scrolling` (implemented, not yet archived) for the scroll-offset rules the wheel relies on, and on `clipboard-file-export` for the `Send to clipboard` entry named in the right-click scenarios.
