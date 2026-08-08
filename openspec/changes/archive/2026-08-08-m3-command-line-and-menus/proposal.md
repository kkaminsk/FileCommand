# M3 — Command line & menus

## Why

M1 gave FileCommand two panels and navigation; M2 added the file operations. The application still has no way to run shell commands, no F9 pull-down menus, no sort control, no drive switching, and no Info panel — the interaction surface that makes NC feel like a working environment rather than a browser. M3 (§9) delivers that surface: an always-live command line backed by the configured shell, the F9 menu system, sort modes, drive select, and Info mode.

## What Changes

- Route printable keys to a command line that shows the active panel's path, and run the typed command via the configured shell with the TUI suspended and a "press any key" return (cmd.exe /C default on Windows for latency; PowerShell/pwsh selectable in `config.toml`).
- Command history navigated with Up/Down while the command line is non-empty, an Alt+F8 history dialog, and the Esc-clears-to-return-cursor rule that hands Up/Down back to the panel.
- Ctrl+Enter pastes the cursor entry's filename and Ctrl+] pastes its path onto the command line.
- Ctrl+O toggles panels on/off, leaving the alternate screen to reveal the host terminal's scrollback of prior command output; any key returns.
- F9 pull-down menu bar (Left / Files / Commands / Options / Right) overlaying the top row, with framed pull-downs, hotkey letters, disabled items, separators, and arrow/Enter/Esc plus left/right menu-to-menu navigation.
- Sort modes Ctrl+F3..F6 (Name/Extension/Time/Size) and Ctrl+F7 (Unsorted) with the header sort-arrow indicator, plus Ctrl+R to re-read the panel.
- Alt+F1/F2 drive-select dialog enumerated via `GetLogicalDrives`, with lazily-fetched volume labels that never block on absent media or slow network drives, error state on selecting an unavailable drive, and UNC support via manual entry.
- Ctrl+L Info display mode: stacked framed boxes (version banner, memory, drive total/free, volume label, serial, file/dir counts) with async values rendering as `…` until their background query resolves.

## Capabilities

### New Capabilities

- `command-line`: The shell prompt showing the active panel path, printable-key routing, Enter-runs-via-shell in suspended-TUI mode, Up/Down history with the Esc-clears rule, Ctrl+Enter/Ctrl+] filename/path paste, Ctrl+O scrollback reveal, and the configurable shell (cmd.exe default vs PowerShell) with its latency tradeoff.
- `pulldown-menus`: The F9 menu-bar overlay and the five Left/Files/Commands/Options/Right menus with framed pull-downs, hotkey letters, disabled items, separators, and arrow/Enter/Esc plus left/right navigation.
- `sort-modes`: Ctrl+F3..F6 sort by Name/Extension/Time/Size, Ctrl+F7 Unsorted, the header sort-arrow indicator, and Ctrl+R re-read.
- `drive-select`: The Alt+F1/F2 drive-select dialog enumerated via `GetLogicalDrives`, with lazily-fetched non-blocking volume labels, unavailable-drive error state, and UNC path support via manual entry.
- `info-panel`: The Ctrl+L Info display mode with stacked framed boxes and async values that render as `…` until their background query completes, then replace in place.

### Modified Capabilities

- None (greenfield project; no existing specs)

## Impact

- **`filecommand-core`**: new `shell` module (build/spawn shell command in the panel directory; cmd.exe/PowerShell selection); `panel` gains Info display mode and sort-mode state; `listing` sort comparators (Name/Extension/Time/Size/Unsorted) and re-read; `config` gains the `shell =` option; command/directory history persistence in `history.json`.
- **`filecommand-tui`**: new `views/` renderers for the command line, the F9 menu bar and pull-downs, the drive-select dialog, and the Info panel; TUI-suspend/restore around shell spawn and Ctrl+O scrollback reveal; input routing for printable keys, history keys, sort keys, and menu navigation.
- **Platform / dependencies**: Windows `GetLogicalDrives`, volume label and free/total-space queries (via `windows`/`windows-sys` or equivalent) on worker threads; no new heavyweight crates. Async drive/volume/count values use the existing worker-thread + event-queue plumbing.
