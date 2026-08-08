## 1. Crate & config setup

- [x] 1.1 Add the `shell = ` key to the `filecommand-core` `config` module, defaulting to `cmd.exe /C` on Windows, accepting `powershell`/`pwsh`, and documenting the ~200 ms+ per-command latency tradeoff in the config schema/comments (§6).
- [x] 1.2 Add config-overridable keybindings for Ctrl+Enter (filename paste) and Ctrl+] (path paste) to the config module so both bindings are remappable.
- [x] 1.3 Extend the persistence layer to store command history in `history.json` alongside the directory frecency store, with an atomic (write-temp-then-rename) writer.
- [x] 1.4 Add the M3 theme roles to the theme table if not already present: `menubar`, `menu.body`, `menu.highlight`, `menu.hotkey`, `menu.disabled`, `info.label`, `info.value`, `info.banner`, and the `panel.header` sort-arrow styling (§4.11).

## 2. Core — shell command construction (`command-line`)

- [x] 2.1 Create the `core::shell` module that builds a shell invocation as `shell + args + user text` plus a working directory (the active panel path), with no terminal dependency, so it is unit-testable (Requirement: Configurable shell — scenarios "Default shell is cmd.exe on Windows", "Configured PowerShell shell is used", "Command construction is terminal-independent").
- [x] 2.2 Have `core::shell` select `cmd.exe /C` by default on Windows and the configured PowerShell/pwsh executable when `shell =` is set.
- [x] 2.3 Expose an "is this entry an executable target" check (PATHEXT match or `.lnk`) so Enter on such an entry routes to the same suspended-spawn path (Requirement: Run command — "Enter on an executable target").

## 3. Core — command-line buffer, routing, history (`command-line`)

- [x] 3.1 Add command-line buffer state to core plus a `core::update` path that appends printable keys to it while a panel is focused and no quick-search/dialog is active (Requirement: prompt & printable-key routing — "Printable key routes to command line").
- [x] 3.2 Implement the single mode-flag arbitration so quick-search mode (§4.7) consumes plain printables and only one typing sink ever sees a given key (scenario "Quick-search mode captures printables instead").
- [x] 3.3 Derive the prompt string from the active panel's current path and update it on Tab/panel-switch and on directory navigation (scenarios "Prompt shows active panel path", "Prompt updates on panel switch").
- [x] 3.4 Implement command-history navigation: Up/Down recall previous/next entries while the buffer is non-empty, and fall through to panel-cursor movement while the buffer is empty (Requirement: Command history — "Up recalls previous command while composing", "Up moves panel cursor when buffer empty").
- [x] 3.5 Implement Esc-clears-buffer semantics that hand Up/Down back to the panel (scenario "Esc clears buffer to release Up/Down to panel").
- [x] 3.6 On command run, append the executed line to history and persist `history.json` atomically (scenario "Executed command persisted to history").
- [x] 3.7 Implement Ctrl+Enter (insert cursor entry filename) and Ctrl+] (insert cursor entry full path) into the buffer (Requirement: Paste filename and path — both scenarios).
- [x] 3.8 Clear the command buffer after a run completes so the prompt shows the active panel path (scenario "Command buffer cleared after run").

## 4. Core — sort modes & re-read (`sort-modes`)

- [x] 4.1 Add per-panel `SortMode` state (Name/Extension/Time/Size/Unsorted) to `core::panel`, defaulting per config, independent for each panel (Requirement: Stable sort — "Sort mode is per-panel").
- [x] 4.2 Implement the sort comparators in `core::listing` (Name, Extension, Time, Size) as stable sorts operating on already-gathered entry metadata with no re-`stat` (Requirement: Stable sort — "Sort is stable for equal keys"; Requirement: Sort-mode keybindings — "Sort operates without re-reading the directory").
- [x] 4.3 Wire Ctrl+F3..F6 and Ctrl+F7 in `core::update` to set the active panel's sort mode and re-sort the current entry list in place (Requirement: Sort-mode keybindings — all four keybinding scenarios).
- [x] 4.4 Compute the header sort-column arrow (`↓`/`↑`) for the current sort key, and no arrow when Unsorted (Requirement: Header sort-column arrow — all three scenarios).
- [x] 4.5 Wire Ctrl+R to re-read the active panel via the existing M1 streaming read path, preserving the current sort mode and surfacing `Reading… N` in the mini-status (Requirement: Re-read the panel — all three scenarios).

## 5. Core — menu state machine (`pulldown-menus`)

- [x] 5.1 Add menu-overlay state to core: bar open/closed, which of the five menus (Left/Files/Commands/Options/Right) is active, and the current pull-down selection index.
- [x] 5.2 Define the five menu item sets with enabled/disabled/separator metadata and per-item hotkey letters (Requirement: Menu contents — "Files menu lists its items", "Left and Right menus mirror each other", "Not-yet-available feature renders disabled"); mark not-yet-built features (Find file, Themes, tabs, Compare) disabled.
- [x] 5.3 Implement F9-open (bar opens, first menu active, its pull-down open) and Esc-close (bar with no pull-down closes) transitions (Requirement: F9 menu-bar overlay — "F9 opens the menu bar", "Esc closes the bar"; Requirement: Menu title activation — "First menu is active when the bar opens").
- [x] 5.4 Implement hotkey-letter jump to a menu while the bar is open (scenario "Hotkey letter jumps to a menu").
- [x] 5.5 Implement vertical selection over enabled items only, skipping disabled items and separators, per NC wrap/stop behavior (Requirement: Vertical navigation — "Arrow keys move selection over enabled items"; Requirement: Pull-down visuals — "Disabled item is skipped").
- [x] 5.6 Implement Enter to dispatch the selected item's action and close the whole overlay, restoring the top row and clock (scenario "Enter activates the selected item and closes the overlay").
- [x] 5.7 Implement Esc-in-pull-down to close the pull-down while keeping the bar open with the active title highlighted (scenario "Esc closes the pull-down but keeps the bar").
- [x] 5.8 Implement Left/Right horizontal traversal that closes the current and opens the adjacent pull-down in one step, wrapping Right↔Left across the five menus (Requirement: Horizontal movement — both scenarios).

## 6. Core — drive enumeration & selection (`drive-select`)

- [x] 6.1 Add drive-select dialog state (target panel, enumerated drive letters, per-drive label slots, selection index) to core.
- [x] 6.2 Implement synchronous drive-letter enumeration via `GetLogicalDrives` behind the fs/platform seam, parseable and unit-testable, with a non-Windows fallback stub (Requirement: invocation & enumeration — "Alt+F1 opens the dialog", "Alt+F2 targets the right panel", "Drive letters appear before any label is known").
- [x] 6.3 Wire Alt+F1/Alt+F2 in `core::update` to open the dialog targeting the left/right panel respectively, and Esc to dismiss it leaving the target panel's directory unchanged (scenarios "Alt+F1 opens…", "Alt+F2 targets…", "Dismissing the dialog").
- [x] 6.4 Model lazy volume-label fetch requests dispatched per drive on the worker-thread → event-queue → `core::update` plumbing, filling each label slot in place on resolution and discarding results for closed/superseded dialogs (Requirement: Lazy non-blocking label fetch — "Label fills in place", "Absent media does not stall", "Slow network drive stays blank", "Stale results are discarded").
- [x] 6.5 Implement drive selection+confirm: switch the target panel to the drive directory on success, or surface the panel inline read-error state when the drive is unavailable (Requirement: Selecting a drive — both scenarios).
- [x] 6.6 Accept manually-entered UNC paths (`\\server\share`) as panel targets with the same non-blocking error handling as local directories (Requirement: UNC path entry — both scenarios).

## 7. Core — Info display mode & async values (`info-panel`)

- [x] 7.1 Add Info to the panel display-mode set with per-panel toggle state (Requirement: Info display mode — "Toggling Info mode on", "Toggling Info mode back off"); wire Ctrl+L in `core::update` to toggle it for the active panel only.
- [x] 7.2 Define the Info content model: version banner (from the shared identity source), memory figure, drive total/free bytes, volume label, serial number, and file/dir counts, each a labelled field (Requirement: Info mode content set — both scenarios; Requirement: Version banner — scenario).
- [x] 7.3 Model async Info values as `…`-until-resolved fields fed by worker-thread queries over the existing event-queue plumbing, replacing in place on resolution (Requirement: Async values fill in place — all three scenarios).
- [x] 7.4 Implement staleness guarding: apply an Info result only if the panel is still in Info mode targeting the same drive+directory, else discard (Requirement: Stale async results — scenario "Result for a changed drive is dropped").

## 8. TUI — command-line view, suspend/restore, Ctrl+O (`command-line`)

- [ ] 8.1 Add a `views/command_line` renderer drawing the prompt path plus buffer on the command-line row (Requirement: prompt & routing — "Prompt shows active panel path").
- [ ] 8.2 Implement the idempotent terminal suspend/restore primitive in the TUI binary: leave raw mode + alternate screen, and re-enter both, safe to call twice, integrated with the app-wide panic hook (Requirement: Run command — "Terminal restored after a failing child").
- [ ] 8.3 Implement Enter-runs-command: suspend the TUI, spawn the core-built child inheriting stdio in the panel directory, wait, prompt "press any key", restore, then re-read the active panel (Requirement: Run command — "Enter runs the typed command", "Command buffer cleared after run"); route Enter on an executable target through the same path.
- [ ] 8.4 Implement Ctrl+O: leave the alternate screen to reveal host scrollback, and re-enter + redraw on any key, keeping no internal output buffer (Requirement: Panels on/off — both scenarios).

## 9. TUI — F9 menu bar & pull-downs (`pulldown-menus`)

- [ ] 9.1 Add a `views/menubar` renderer overlaying the top row: full-width black-on-cyan bar with the five titles, hotkey letters in `menu.hotkey`, and the clock suppressed while open (Requirement: F9 menu-bar overlay — "F9 opens…", "Menu hotkey letters are highlighted"; restore top row + clock on close).
- [ ] 9.2 Render the active menu title white-on-black (`menu.highlight`) (Requirement: Menu title activation — "First menu is active").
- [ ] 9.3 Render the open pull-down as a single-line CP437-framed box below its title: selected item white-on-black, enabled items black-on-cyan, disabled grey (white-on-cyan `menu.disabled`), separators drawn with `─` (Requirement: Pull-down visuals — "Framed pull-down with a selected item", "Separator row rendering").

## 10. TUI — drive-select dialog & Info panel views

- [ ] 10.1 Add a `views/drive_select` renderer painting all enumerated drive letters on the first frame with blank label columns, filling labels in place as they resolve, using CP437 glyphs and ANSI-16 colors only (Requirement: enumeration — "Drive letters appear before any label"; Requirement: lazy fetch — "Label fills in place").
- [ ] 10.2 Add a `views/info_panel` renderer: vertically stacked single-line-framed boxes inside the panel's double border, `info.banner` bright-white banner, `info.label` cyan labels, `info.value` bright-yellow values, and `…` static placeholders with no spinner/animation (Requirement: Info display mode — "Boxes are single-line framed and stacked"; Requirement: field labels distinct; Requirement: Info mode rendering uses static text only — scenario "No animation for pending values").

## 11. TUI — input routing & wiring

- [ ] 11.1 Route printable keys, Up/Down (history vs cursor), Esc, Ctrl+Enter, Ctrl+] into the command-line update path with the quick-search mode-flag arbitration.
- [ ] 11.2 Route F9 and menu-navigation keys (arrows, Enter, Esc, hotkey letters, Left/Right) into the menu state machine, and dispatch activated menu items to their M3/earlier actions.
- [ ] 11.3 Route Ctrl+F3..F7, Ctrl+R, Alt+F1/F2, and Ctrl+L to their core update paths, and dispatch menu items that mirror these actions (display mode, sort mode, re-read, drive select, on/off, quit) through the same handlers.

## 12. Testing (§8)

- [x] 12.1 Core unit tests: shell command construction for default cmd.exe and configured PowerShell, and working-directory selection, with no terminal (`command-line` shell requirement scenarios).
- [x] 12.2 Core unit tests: command-line routing vs quick-search arbitration, prompt update on panel switch, history Up/Down vs empty-buffer cursor movement, Esc-clears, and history persistence to `history.json` (all `command-line` routing/history scenarios).
- [x] 12.3 Property tests (proptest): the four sort comparators for stability and total-order correctness, plus per-panel independence (§8; `sort-modes` stable-sort scenarios).
- [x] 12.4 Core unit tests: sort-mode keybindings re-sort in place with no re-read, header arrow computation, and Ctrl+R re-read preserving sort mode (`sort-modes` keybinding, header, and re-read scenarios).
- [x] 12.5 Core unit tests: menu state machine — open/close, hotkey jump, vertical selection skipping disabled/separators, Enter-dispatch-and-close, Esc-keeps-bar, horizontal wrap, and menu contents/disabled sets (all `pulldown-menus` scenarios).
- [x] 12.6 Core unit tests: drive enumeration parsing behind the fs seam, Alt+F1/F2 targeting, lazy-label fill-in-place, stale-result discard, available/unavailable selection outcomes, and UNC handling (all `drive-select` scenarios).
- [x] 12.7 Core unit tests: Info toggle per-panel isolation, content set, async placeholder→value replacement, and stale-result discard on drive/directory change (all `info-panel` state scenarios).
- [ ] 12.8 TUI snapshot tests (insta + `TestBackend`, pinned `Clock`/size/locale): command line with prompt and recalled history entry (Requirement: prompt & routing).
- [ ] 12.9 TUI snapshot tests: F9 menu bar with an open pull-down showing a selected item, a disabled item, and a separator (`pulldown-menus` visuals).
- [ ] 12.10 TUI snapshot tests: drive-select dialog before labels resolve (blank columns) and after labels resolve (`drive-select` scenarios).
- [ ] 12.11 TUI snapshot tests: Info panel with `…` placeholders and with resolved values, and a panel header showing the sort arrow (`info-panel` and `sort-modes` header scenarios).
- [ ] 12.12 TUI test: terminal restore is idempotent and the panic hook restores raw mode + alternate screen after a failing/panicking child (Requirement: Run command — "Terminal restored after a failing child").
