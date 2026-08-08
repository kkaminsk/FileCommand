# M3 — Command Line & Menus Build Task

You are building **Milestone 3 (Command Line & Menus)** of FileCommand, a Rust keyboard-driven dual-panel terminal file manager recreating Norton Commander 5.5.

## Project Context

M1 (Shell) and M2 (File Operations) are already implemented and working. The codebase has:
- `filecommand-core`: modules `clock`, `config`, `identity`, `listing`, `panel`, `theme`, `update`, `fs_ops/` (conflict, dialog, error, fs, job, path, worker)
- `filecommand-tui`: `app`, `clock`, `input/`, `layout`, `style`, `terminal`, `worker`, `views/` (cmdline, keybar, panel, placeholder, quit_dialog, splash, conflict_dialog, delete_confirm, destination_input, error_dialog, progress_dialog, skipped_summary)
- 180 tests passing
- Data flow: `key press → Command → core::update(state, cmd) → (state, Vec<Effect>)` with worker threads

## What You Must Build

Implement M3: the command line, F9 pull-down menus, sort modes, drive select, and Info panel.

### Tasks

#### 1. Config setup
- Add `shell =` key to config (default `cmd.exe /C` on Windows; accepts `powershell`/`pwsh`)
- Add config-overridable keybindings for Ctrl+Enter and Ctrl+]
- Extend persistence to store command history in `history.json` (atomic write)
- Add M3 theme roles if not present: `menubar`, `menu.body`, `menu.highlight`, `menu.hotkey`, `menu.disabled`, `info.label`, `info.value`, `info.banner`

#### 2. Core — shell command construction
- Create `core::shell` module building shell invocation as `shell + args + user text` + working directory (active panel path), no terminal dependency, unit-testable
- Select `cmd.exe /C` by default on Windows, configured PowerShell/pwsh when `shell =` is set
- Expose "is this entry an executable target" check (PATHEXT match or `.lnk`)

#### 3. Core — command-line buffer, routing, history
- Add command-line buffer state to core; append printable keys while panel focused and no dialog/quick-search active
- Mode-flag arbitration: quick-search mode consumes plain printables; only one typing sink sees a given key
- Derive prompt string from active panel's current path; update on Tab/panel-switch/navigation
- Command history: Up/Down recall while buffer non-empty; fall through to panel-cursor when empty; Esc clears buffer
- On command run: append to history, persist `history.json` atomically
- Ctrl+Enter inserts cursor entry filename; Ctrl+] inserts cursor entry path
- Clear command buffer after run completes

#### 4. Core — sort modes & re-read
- Add per-panel `SortMode` state (Name/Extension/Time/Size/Unsorted), independent per panel
- Implement sort comparators in `core::listing` as stable sorts on already-gathered metadata (no re-stat)
- Wire Ctrl+F3..F6 and Ctrl+F7 to set sort mode and re-sort in place
- Compute header sort-column arrow (↓/↑) for current sort key; no arrow when Unsorted
- Wire Ctrl+R to re-read panel via existing streaming read path, preserving sort mode

#### 5. Core — menu state machine
- Add menu-overlay state: bar open/closed, active menu (Left/Files/Commands/Options/Right), pull-down selection index
- Define five menu item sets with enabled/disabled/separator metadata and hotkey letters; mark not-yet-built features disabled
- F9 opens bar (first menu active, pull-down open); Esc closes bar
- Hotkey-letter jump to menu while bar open
- Vertical selection over enabled items only, skipping disabled/separators
- Enter dispatches selected item action and closes overlay
- Esc in pull-down closes pull-down but keeps bar open
- Left/Right horizontal traversal between menus with pull-down staying open, wrapping

#### 6. Core — drive enumeration & selection
- Add drive-select dialog state (target panel, drive letters, per-drive label slots, selection index)
- Synchronous drive-letter enumeration via `GetLogicalDrives` behind fs/platform seam; non-Windows fallback stub
- Alt+F1/F2 open dialog targeting left/right panel; Esc dismisses
- Lazy volume-label fetch on worker threads; fill in place on resolution; discard stale results
- Drive selection: switch panel to drive directory on success, surface panel read-error on unavailable drive
- Accept manually-entered UNC paths

#### 7. Core — Info display mode & async values
- Add Info to panel display-mode set with per-panel toggle; Ctrl+L toggles for active panel
- Info content model: version banner (from identity), memory, drive total/free, volume label, serial, file/dir counts
- Async values as `…`-until-resolved fed by worker-thread queries; replace in place on resolution
- Staleness guarding: apply result only if panel still in Info mode targeting same drive+directory

#### 8. TUI — command-line view, suspend/restore, Ctrl+O
- Add `views/command_line` renderer drawing prompt path + buffer
- Implement idempotent terminal suspend/restore primitive: leave raw mode + alternate screen, re-enter both, safe to call twice
- Enter-runs-command: suspend TUI, spawn child inheriting stdio in panel directory, wait, "press any key" return, restore, re-read panel
- Ctrl+O: leave alternate screen to reveal host scrollback; re-enter + redraw on any key

#### 9. TUI — F9 menu bar & pull-downs
- Add `views/menubar` renderer: full-width black-on-cyan bar with five titles, hotkey letters in `menu.hotkey`, clock suppressed while open
- Active menu title white-on-black (`menu.highlight`)
- Pull-down: single-line CP437-framed box below title; selected item white-on-black, enabled black-on-cyan, disabled grey, separators with `─`

#### 10. TUI — drive-select dialog & Info panel views
- `views/drive_select` renderer: drive letters immediately, blank label columns filling in place
- `views/info_panel` renderer: stacked single-line-framed boxes, `info.banner` bright-white, `info.label` cyan, `info.value` bright-yellow, `…` placeholders with no animation

#### 11. TUI — input routing & wiring
- Route printable keys, Up/Down (history vs cursor), Esc, Ctrl+Enter, Ctrl+] into command-line update path with quick-search arbitration
- Route F9 and menu-navigation keys into menu state machine; dispatch activated items
- Route Ctrl+F3..F7, Ctrl+R, Alt+F1/F2, Ctrl+L to core update paths

#### 12. Testing
- Core unit tests: shell command construction for cmd.exe and PowerShell, working-directory selection
- Core unit tests: command-line routing vs quick-search arbitration, prompt update, history Up/Down, Esc-clears, history persistence
- Property tests: four sort comparators for stability and total-order; per-panel independence
- Core unit tests: sort-mode keybindings, header arrow, Ctrl+R re-read
- Core unit tests: menu state machine — open/close, hotkey jump, vertical selection, Enter-dispatch, Esc-keeps-bar, horizontal wrap
- Core unit tests: drive enumeration, Alt+F1/F2 targeting, lazy-label fill, stale-result discard, UNC handling
- Core unit tests: Info toggle isolation, content set, async placeholder→value, stale discard
- TUI snapshot tests: command line with prompt, F9 menu bar with pull-down, drive-select dialog, Info panel with placeholders and resolved values, header sort arrow

## Theme roles (from §4.11)
- `menubar` / `menu.body`: black on cyan
- `menu.highlight`: white on black
- `menu.hotkey`: bright-yellow on cyan
- `menu.disabled`: white on cyan
- `info.label`: cyan on blue
- `info.value`: bright-yellow on blue
- `info.banner`: bright-white on blue
- `clock`: black on cyan (hidden while menu open)

## Constraints
1. `filecommand-core` MUST NOT depend on ratatui/crossterm
2. `core::update` is pure — no I/O, no threads, no terminal side effects
3. All rendering uses theme roles — no hardcoded colors
4. CP437 glyphs only — no emoji, no Nerd Font
5. Default shell is `cmd.exe /C` for latency; PowerShell is opt-in
6. Git workflow: Work on `build/m3-command-line` branch. Commit when done.

## Success Criteria
- `cargo build` compiles
- `cargo test` passes all tests (existing + new)
- Binary runs with command line, F9 menus, sort modes, drive select, Info mode

Start by reading the existing code, then build M3 on top. Run cargo build and cargo test. Commit when done.