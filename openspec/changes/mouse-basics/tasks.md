# Tasks: mouse-basics

## 1. Terminal lifecycle

- [ ] 1.1 `[mouse] enabled` in `crates/filecommand-core/src/config.rs` (default true) and the `--nomouse` launch flag (mouse-input: "Mouse capture configuration")
- [ ] 1.2 `TerminalGuard::new`/`suspend`/`resume` enable and disable capture in the right order; `restore_terminal()` disables unconditionally (application-shell: "Terminal ownership and restoration on every exit"; "Panic hook restores the terminal before reporting")
- [ ] 1.3 Extend `crates/filecommand-tui/tests/panic_restoration.rs` and add a suspend/resume ordering test

## 2. Hit map and mapping

- [ ] 2.1 `crates/filecommand-tui/src/hitmap.rs`: `HitMap`, `PanelHits`; `views::render` returns it; panel, keybar, menubar, and dialog views record rects (mouse-input: "Hit-testing stays in the TUI")
- [ ] 2.2 `input::map_mouse` and `MouseTracker` (press bookkeeping, double-click timing, Ctrl-click without movement) (mouse-input: "Click focuses and places the cursor"; "Double-click acts as Enter"; "Ctrl+click toggles selection")
- [ ] 2.3 Core commands `ClickEntry`, `FocusPanel`, `ScrollPanel`, `KeybarPress`, `MenuTitleClick`, `MenuItemClick`, `DialogButtonClick`, `OpenActionMenuAt` and their reducer arms in `update.rs`; `ScrollPanel` moves the cursor three rows so the existing scroll-offset clamp follows; viewer wheel through `ViewerInput::ScrollLines(±3)`, editor wheel as three `EditorMove` steps (mouse-input: "Click focuses and places the cursor"; "Wheel moves the cursor of the panel under the pointer"; "Key bar, menu bar, pull-down items, and dialog buttons are clickable")
- [ ] 2.4 Event-loop drain/coalesce in `app.rs`: `Moved` discarded, wheel notches summed (mouse-input: "Mouse events are coalesced")
- [ ] 2.5 Mode-gating table in `map_mouse` (mouse-input: "Mouse is honoured only where the key would be")

## 3. Right-click and menus

- [ ] 3.1 Action menu for directories (no View, Edit, or Run) and selection-scoped invocation (file-action-menu: "Directory targets and selection-scoped invocation")
- [ ] 3.2 Right-click routing through `OpenActionMenuAt` (mouse-input: "Right-click opens the action menu")
- [ ] 3.3 Help `Mouse` topic: `HELP_TOPICS` grows to 11, `topic_page_text` arms and index-based tests renumber, page text written (help-and-about: "Help topic list")

## 4. Verification

- [ ] 4.1 Core tests for every new command; `map_mouse` tests with literal `MouseEvent`s; proptest that hit-map row rects nest inside their panel and never overlap across terminal sizes, splits, and scroll offsets
- [ ] 4.2 Manual matrix on Windows Terminal, conhost, WezTerm, and GNOME Terminal: click, double-click, wheel, right-click, Ctrl+click, shell run and return
- [ ] 4.3 Existing snapshot goldens byte-identical except the Help topic-list golden (re-pinned deliberately with its one new row); `cargo build --workspace` and `cargo test --workspace` green
