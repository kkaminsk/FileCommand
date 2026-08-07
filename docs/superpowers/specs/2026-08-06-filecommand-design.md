# FileCommand — Application Specification

**Date:** 2026-08-06
**Status:** Approved design, pre-implementation
**Repository:** https://github.com/kkaminsk/FileCommand

## 1. Overview

FileCommand is a keyboard-driven, dual-panel file manager for the terminal, written in Rust. It faithfully recreates the look, layout, and core workflow of Norton Commander 5.5 (1998) — classic blue/cyan palette, double-line panel frames, F-key bar, and an always-available command line — while adding a small set of modern extras: full UTF-8 and Windows long-path support, quick filter and fuzzy directory jump, panel tabs, git-aware panel info, and configurable themes.

**Primary platform:** Windows (Windows Terminal / PowerShell console). The codebase stays cross-platform via `crossterm`; Linux/macOS should build and run, but only Windows is tested and supported in v1.

### Reference material

- `research/images/` — eight screenshots of Norton Commander 5.5/5.51 (panels, viewer text/hex modes, info panel, help dialog, sort-mode key bar) from [Abandonware DOS](https://www.abandonwaredos.com/abandonware-game.php?abandonware=Norton+Commander+5.5&gid=1814), [WinWorld](https://winworldpc.com/product/norton-commander/55x), and [Wikipedia](https://en.wikipedia.org/wiki/Norton_Commander). (Local copies are git-ignored.)
- [UI Museum: Norton Commander 5.0](https://ilyabirman.net/meanwhile/all/ui-museum-norton-commander-5-0/) — detailed UI behavior reference.
- [WinNc: Norton Commander keyboard shortcuts](https://www.winnc.com/norton_commander_keyboard_shortcuts/).

## 2. Goals and non-goals

### Goals (v1)

1. Everyday file management entirely from the keyboard: navigate, select, copy, move, rename, delete, mkdir, view, edit.
2. Authentic NC 5.5 visual experience by default (colors, layout, chrome, dialogs).
3. Modern conveniences that do not disturb the classic workflow: quick filter, fuzzy jump, tabs, git info, themes, UTF-8, long paths.
4. Safe operations: confirmations, per-file error recovery (Retry/Skip/Abort), cancellable long-running operations with progress.
5. Fast startup (< 200 ms to first paint) and responsive UI on directories with 100k+ entries. First paint is decoupled from the first listing: the initial directory listing streams in after paint (a 100k-entry directory fills progressively), and git/drive info is always async and never gates paint.

### Non-goals (v1)

- Archive virtual file system (browsing ZIPs as directories)
- FTP / Commander Link / networking
- File split/merge, printing
- NCD tree as a primary navigation replacement (a Tree *panel mode* is in scope; the full-screen NCD workflow is not)
- Mouse-first workflows (basic mouse click/scroll support is nice-to-have, not required)

## 3. Architecture

Cargo workspace with two crates:

```
filecommand/
├── Cargo.toml            (workspace)
├── crates/
│   ├── filecommand-core/     library crate — no terminal dependencies
│   └── filecommand-tui/      binary crate — ratatui + crossterm
```

### 3.1 `filecommand-core`

Platform-agnostic application logic. No dependency on ratatui/crossterm; fully unit-testable.

Modules:

- **`panel`** — panel state machine: current directory, entry list, cursor, selection set, sort mode, filter, display mode (Brief/Full/Info/Tree/QuickView), per-panel tab list and active tab.
- **`fs_ops`** — file operations engine. Copy/move/delete/mkdir implemented as cancellable jobs running on a worker thread, emitting progress events (current file, bytes done/total, files done/total) and error events that block awaiting a Retry/Skip/Abort/SkipAll decision. Handles overwrite conflicts (Overwrite/Skip/Rename/All variants), read-only attributes, and Windows long paths (`\\?\` prefix handling behind an abstraction — chosen over relying on the Windows 10 1607+ `LongPathsEnabled` registry opt-in plus manifest `longPathAware` declaration, since the `\\?\` prefix works unconditionally without requiring the user to have that machine-wide setting enabled). Copy preserves alternate data streams (Rust's `std::fs::copy` on Windows corresponds to `CopyFileEx`, and the standard library documents that alternate NTFS streams are copied). `CopyFileEx` also preserves attributes and timestamps as part of its normal Win32 semantics, though this isn't spelled out in Rust's own documentation — verify with an integration test in M2 rather than assuming. Same-volume move is a `rename` (instant); cross-volume move is copy-then-delete, with the delete performed only after a verified copy. Case-only renames (`foo` → `Foo`) must succeed — the target-exists check is file-identity-aware, not name-comparison-based. All file system access goes through a narrow internal trait so tests can deterministically inject failures (permission denied, sharing violation, disk full).
- **`listing`** — directory reading, sorting (Name, Extension, Time, Size, Unsorted), filtering (wildcard filter and quick-filter substring), attribute/metadata gathering. Async/streamed for large directories. Entry names are stored as `OsString`/`PathBuf` (Windows names are UTF-16 and may contain unpaired surrogates); all fs operations use the original `OsString`. Display uses lossy conversion with a visual marker; control and zero-width characters are replaced for rendering, and column layout uses grapheme/display width (`unicode-width`) so CJK and emoji names align. On Windows, per-entry metadata comes from the directory enumeration itself (`FindFirstFile` data via `DirEntry::metadata`) — no per-file `stat` calls.
- **`quicksearch`** — type-ahead jump within a panel; fuzzy directory-jump index (frecency-ranked list of visited directories, persisted).
- **`git_info`** — detects enclosing git repository; provides current branch name and per-file status markers (modified/untracked/staged). Uses `git2` (libgit2) on a dedicated worker thread — libgit2 status calls are blocking and not cancellable mid-call, so "timeout" means the result is discarded if stale and the repo is marked "no info" for the session (the abandoned thread is left to finish). Status queries are pathspec-scoped to the panel's directory, not repo-wide, and untracked-directory contents are not enumerated. Degrades to "no info" silently. (Fallback candidates if libgit2 proves too heavy: `gitoxide`, or a `git status --porcelain` subprocess.)
- **`config`** — loads/saves `config.toml` and theme files from the platform config directory (`%APPDATA%\FileCommand` on Windows). Includes keybinding overrides, editor/viewer external commands, confirmation toggles, panel defaults. The config schema carries a `version =` key; unknown keys produce warnings, never hard failures.
- **`theme`** — named 16-color-style theme model (role → color mapping: panel frame, directory text, file text, selected, cursor, key bar, dialogs, viewer). Ships with built-in `nc-classic` (default, matching the screenshots), plus `nc-mono` (black/white). User themes are TOML files.
- **`shell`** — command-line passthrough: builds and spawns the user's shell command in the panel's current directory, suspending the TUI while it runs. Default shell on Windows is `cmd.exe /C` for latency (spawning PowerShell costs 200 ms+ per command, which kills the instant NC feel); `config.toml` `shell =` may select PowerShell/pwsh, with the latency tradeoff documented.

### 3.2 `filecommand-tui`

The binary. Owns the terminal, the event loop, and all rendering.

- **Event loop:** crossterm input events + core job events → update core state → redraw. Target: single-threaded UI with worker threads for fs jobs, directory reads, and git status.
- **`views/`** — one renderer per screen region/state: panels (all display modes), F-key bar (base and Ctrl/Alt variants), command line, pull-down menus, dialogs (message, input, confirm, progress, error), viewer, editor, help.
- **`input/`** — maps key events to commands via the (configurable) keymap; routes to focused component (panel, dialog, menu, viewer, editor, command line).

### 3.3 Data flow

```
key press → input map → Command → core::update(state, cmd) → new state
                                        ↘ spawn fs job (worker thread)
worker events (progress/error/done) → event queue → core::update → redraw
```

Rendering is a pure function of core state. All state mutations flow through `core::update`, which is what unit tests drive.

## 4. User interface specification

Layout matches the reference screenshots.

The UI reflows on terminal resize events. Minimum supported size is 80×24; below that, a "terminal too small" placeholder is drawn instead of the panels.

### 4.1 Screen layout (top to bottom)

1. **Panels** — two side-by-side panels filling most of the screen. Each panel:
   - Double-line border; path centered in the top border, shown inverse (black on cyan) for the active panel.
   - Column header row per display mode; the sort column shows a `↓`/`↑` indicator next to its label (e.g. `C:↓ Name`).
   - Entry rows: directories in UPPERCASE bright white style, files in lowercase-styling per theme; `▶UP--DIR◀` for `..`, `▶SUB-DIR◀` in the Size column for directories. Selected entries render yellow; the cursor is an inverse bar.
   - **Mini-status line** at the panel bottom (inside the border): current entry's name, size, date, time; when files are selected, shows `N files selected, X bytes` instead. Selected directories contribute 0 bytes to the total (classic NC behavior — directories are not sized in v1).
   - **Tab strip**: when a panel has more than one tab, a single compact row above the panel shows numbered tab labels; hidden with one tab (preserves classic look).
   - A clock (`h:mm a` style) in the top-right corner of the screen.
2. **Command line** — shell prompt with the active panel's path (`C:\NORTON>_`). Printable keys typed while a panel is focused go to the command line, exactly like NC. Enter runs the command via the shell (TUI suspends, output shown, "press any key" to return). Ctrl+Enter pastes the current filename to the command line; Ctrl+] pastes the current path. History with Up/Down when the command line is non-empty — this means the panel cursor cannot be moved mid-composition; Esc clears the command line to return Up/Down to the panel. Alt+F8 opens a history dialog.
3. **F-key bar** — one row, ten slots: `1Help 2Menu 3View 4Edit 5Copy 6RenMov 7Mkdir 8Delete 9PullDn 10Quit`. Holding Ctrl or Alt relabels the bar live to the modifier's meanings (e.g. Ctrl: `3Name 4Exten 5Time 6Size 7Unsort…`), as seen in the screenshots. **Feasibility gate:** live relabeling requires standalone modifier press/release events. Crossterm's Windows event parser explicitly drops bare `VK_SHIFT`/`VK_CONTROL`/`VK_MENU` console records (confirmed against crossterm's current source; unchanged since at least 2022) — modifier state only ever arrives attached to another key's event. This holds regardless of the hosting terminal: Windows Terminal added Kitty Keyboard Protocol support in February–March 2026 (PR [microsoft/terminal#19817](https://github.com/microsoft/terminal/pull/19817), shipped in Preview v1.25.622.0), but crossterm's Windows backend still hardcodes its keyboard-enhancement-flags path to fail unconditionally on Windows (crossterm issue [#1022](https://github.com/crossterm-rs/crossterm/issues/1022)), so it cannot exploit that terminal capability yet. This feature is best-effort and gated on an M1 spike verifying whether bare modifier events are obtainable some other way (a crossterm release that unlocks KKP on Windows, or a direct `ReadConsoleInput` call bypassing crossterm's parser). Fallback if unavailable: relabeling is dropped and the Ctrl/Alt variants are documented in F1 Help instead.

### 4.2 Panel display modes

- **Brief** — three columns of names only.
- **Full** (default) — Name | Size | Date | Time.
- **Info** — replaces the panel with system/drive/directory info (version banner, memory, drive total/free bytes, volume label, serial, file/dir counts), mirroring the Info panel screenshot.
- **Tree** — lazily-expanded directory tree of the current drive (directories are read on expand, not by scanning the whole drive up front). Moving the cursor updates the *opposite* panel to list the highlighted directory; Enter returns *this* panel to its previous list mode at the chosen directory.
- **Quick View** — shows a preview (text head) of the file under the opposite panel's cursor.

### 4.3 Pull-down menus (F9)

Five menus: **Left**, **Files**, **Commands**, **Options**, **Right**. Navigation with arrows/Enter/Esc; hotkey letters highlighted.

- **Left/Right** — display mode, sort mode, filter, re-read, drive select, new/close tab (mirror of each other).
- **Files** — View, Edit, Copy, Rename/Move, Make directory, Delete, Attributes (read-only/hidden/archive toggle dialog), Select group / Deselect group / Invert (also `+`/`-`/`*` keys), Quit.
- **Commands** — Find file, History, Swap panels, Panels on/off, Compare directories, Fuzzy jump, Menu file edit (user menu).
- **Options** — Configuration (confirmations, panel options, show hidden/system files — default off, matching NC; hidden entries render dimmed when shown), Themes, Editor selection, Save setup.

### 4.4 Dialogs

NC-style modal boxes: cyan-on-blue framed dialogs; nested/secondary dialogs use the alternate (grey) style; input fields rendered with bracket-and-dots styling; buttons as highlighted labels (e.g. yellow) navigated with Tab/arrows, activated with Enter, dismissed with Esc. Standard dialogs: message, confirm (Yes/No), input (mkdir, rename target, copy destination pre-filled with opposite panel path), overwrite conflict, error Retry/Skip/Abort, progress (file counts, byte progress bar, current path, Cancel).

### 4.5 Viewer (F3)

Built-in read-only viewer with two modes, switchable via F4-in-viewer (label toggles `Hex`/`ASCII` as in the screenshots):

- **Text mode** — UTF-8 with lossy fallback; wrap/unwrap toggle (F2); search (F7); Col/offset, size, and percent indicators in the header.
- **Hex mode** — classic offset | hex bytes | ASCII gutter layout.

Streams from disk; must open multi-GB files instantly. The viewer memory-maps or chunk-reads and builds no full line index: the percent indicator is byte-offset-based; backward navigation scans backward from the current offset for line starts, with a max-line-length cap (e.g. 64 KB) after which lines are hard-split; search streams with overlap at chunk boundaries; hex mode is pure offset math. Viewer F-key bar: `1Help 2Unwrap 4Hex 7Search 10Quit`.

### 4.6 Editor (F4)

Built-in minimal editor for quick edits: insert/overwrite, cut/copy/paste line-based selection, search/replace, undo, save (F2), UTF-8 + CRLF/LF preservation, "modified" indicator and save-on-exit prompt. `config.toml` may set an external editor command; when set, F4 suspends the TUI and launches it. The built-in editor targets files < 10 MB (larger files open in the viewer with a notice).

To contain scope, the v1 built-in editor floor is explicitly minimal: single-level undo batch, line-based selection only, no regex in search/replace. The external-editor hook ships first (M4) so F4 is useful before the built-in editor lands (M5).

### 4.7 Modern extras

- **Quick filter (Ctrl+P)** — inline input in the mini-status line; panel narrows to substring matches as you type; Esc clears. Note: Ctrl+P intentionally deviates from classic NC (where it toggles the inactive panel — a feature not implemented in v1); the binding is overridable in `config.toml`.
- **Type-ahead jump** — Alt+letter starts quick-search mode and moves the cursor to the first match; while active, *plain* printable keys extend the search pattern (shown in the mini-status line), Backspace shortens it, and Esc/movement keys exit, returning printable keys to the command line (this resolves the conflict with command-line typing in §4.1).
- **Fuzzy jump (Ctrl+J)** — dialog with a fuzzy-matched, frecency-ranked list of previously visited directories; Enter navigates the active panel there. Directory history persists across sessions.
- **Panel tabs (Ctrl+T new, Ctrl+W close, Alt+1..9 switch)** — independent directory+state per tab.
- **Git info** — inside a repository, the active panel's top border shows ` (branch-name)` after the path, and a one-cell status marker column (`M`/`?`/`+`) appears before file names. Silent and absent outside repositories or on timeout.
- **Themes** — `theme = "nc-classic"` in config; theme TOML files in the config directory; Options → Themes switches at runtime.

## 5. Keyboard reference (v1 required set)

| Key | Action |
|---|---|
| F1 | Help screen |
| F2 | User menu (from `usermenu.toml`) |
| F3 / F4 | View / Edit file under cursor |
| F5 / F6 | Copy / Rename-Move (destination dialog pre-filled with opposite panel path) |
| F7 / F8 | Make directory / Delete |
| F9 / F10 | Pull-down menus / Quit (with confirmation) |
| Tab | Switch active panel |
| Ins | Toggle selection, cursor advances |
| + / - / * | Select group / deselect group (wildcard dialog) / invert selection |
| Enter | Enter directory; run executable; run command line if non-empty |
| Ctrl+PgUp / Backspace-on-empty-cmdline | Parent directory |
| Alt+F1 / Alt+F2 | Drive select for left / right panel |
| Ctrl+F3..F6 | Sort by Name / Extension / Time / Size; Ctrl+F7 Unsorted |
| Ctrl+O | Panels on/off (full-screen command output view) |
| Ctrl+U | Swap panels |
| Ctrl+R | Re-read panel |
| Ctrl+L | Info panel toggle |
| Ctrl+Q | Quick View mode toggle |
| Ctrl+Enter / Ctrl+] | Paste filename / path to command line |
| Alt+F7 | Find file |
| Alt+F8 | Command history |
| Ctrl+P / Ctrl+J | Quick filter / fuzzy directory jump *(modern)* |
| Ctrl+T / Ctrl+W / Alt+1..9 | New tab / close tab / switch tab *(modern)* |

All bindings are overridable in `config.toml`; the table above is the default map.

Notes:

- **Ctrl+Enter** is guaranteed on Windows because crossterm reads native console input records directly there, independent of ANSI/kitty-protocol support. On other platforms it requires a terminal supporting the kitty keyboard protocol plus crossterm's `PushKeyboardEnhancementFlags`; otherwise the binding is unavailable. Note: Windows Terminal itself gained native Kitty Keyboard Protocol support in February–March 2026, but crossterm's Windows backend currently fails that enhancement-flags call unconditionally regardless of terminal capability (crossterm issue [#1022](https://github.com/crossterm-rs/crossterm/issues/1022)) — this doesn't affect Windows (native console reading is unaffected and needs no protocol), but it means Windows Terminal's new capability can't yet be used through crossterm on Windows either, so the native-console-record path remains the only reliable source of Ctrl+Enter detection there. Ctrl+] works everywhere (it's ASCII 0x1D).
- **Ctrl+O** leaves the alternate screen, revealing the terminal's scrollback containing prior command output; any key returns. Output history is whatever the host terminal retains — FileCommand does not maintain its own output buffer.
- **Enter on an executable** (Windows): resolves PATHEXT extensions and `.lnk` shortcuts, and runs via the shell in suspended-TUI mode, same as a command-line command.
- **Alt+F1/F2 drive list**: enumerated via `GetLogicalDrives`, with volume labels fetched lazily; drives with no media (A:) or slow network drives must not block the dialog — selecting an unavailable drive shows the panel error state. UNC paths are supported via manual entry on the command line and in copy/move destination dialogs.
- M1 includes a key-delivery matrix test in Windows Terminal and conhost; any undeliverable default binding gets a documented alternate.

## 6. Configuration

`%APPDATA%\FileCommand\` (Windows) / XDG config dir (other platforms):

- `config.toml` — general options (confirmations, panel defaults, editor command, theme name, clock format), keybinding overrides.
- `themes/*.toml` — user themes.
- `usermenu.toml` — F2 user menu entries (label + command).
- `history.json` — command history, directory frecency data (written atomically).

Missing files are created with defaults on first run. A malformed config produces a startup warning dialog and falls back to defaults; it is never silently overwritten.

## 7. Error handling and file system semantics

- **File operation errors** (permission denied, path too long, sharing violation, disk full): job pauses and raises Retry / Skip / Skip All / Abort. Skipped files are listed in a summary dialog at job end.
- **Overwrite conflicts:** Overwrite / Skip / Rename / Overwrite All / Skip All, with source vs target size/date shown.
- **Delete:** confirmation dialog (single item names the item; multi-selection shows count); deleting non-empty directories requires a second confirmation. No recycle bin in v1 — deletes are permanent, and the confirmation says so.
- **Reparse points (symlinks, junctions):** shown with a marker; Enter follows them. Delete removes the link itself, never the target's contents. Copy copies the link target's *content* by default (NC-era behavior), with recursion-cycle protection (visited-ID set); recursive operations never traverse into junctions pointing inside the source tree.
- **Refresh policy:** panels re-read automatically after FileCommand's own operations complete. No filesystem watching in v1 — Ctrl+R is the manual refresh. (A `notify`-based watcher is a v2 candidate.)
- **Panel read errors** (drive removed, access denied): panel shows an inline error state and offers re-read/drive change; the app never crashes on fs errors.
- **Git/large-directory timeouts:** background providers degrade to absent info; never block input.
- **Panic policy:** panic hook restores the terminal (leave raw mode/alternate screen) before printing the report.

## 8. Testing strategy

- **`filecommand-core` unit tests:** fs_ops against temp directories (copy/move/delete trees, conflicts, cancellation mid-job, error injection via the fs trait seam); sorting/filtering; selection semantics; quick-search and fuzzy ranking; config/theme parsing including malformed input; git_info against fixture repos; non-Unicode `OsString` filename handling.
- **Property-based tests** (proptest): sort comparators, the overwrite-conflict-resolution state machine, and path joining including `\\?\` prefixing.
- **TUI snapshot tests:** ratatui `TestBackend` renders key screens (Full panel, Brief, Info, dialogs, viewer text/hex, F-key bar modifier variants) compared against committed snapshots via the `insta` crate (ratatui's own documented recipe for `TestBackend`-based snapshot testing). The TUI takes an injected time source (`Clock` trait) so the on-screen clock is pinnable; snapshot tests pin time, terminal size, and locale, and fixture directories use fixed timestamps.
- **Integration smoke test:** scripted event sequence (navigate, select, copy, verify result on disk) run in CI.
- **CI platforms:** the full suite runs on `windows-latest` (the only supported platform); Linux CI builds and runs core tests only, as a compile guard for the cross-platform claim.
- **Manual test checklist:** Windows Terminal rendering (colors, box-drawing glyphs), long-path (> 260 chars) operations, a 100k-entry directory, a multi-GB file in the viewer.

## 9. Milestones

1. **M1 — Shell:** workspace scaffolding, event loop, theme system, two panels rendering a real directory (Full mode), navigation, Tab, F10 quit. Includes three de-risking spikes: (a) key-delivery matrix in Windows Terminal and conhost, (b) standalone modifier press/release detection (gates F-key bar live relabeling, §4.1), (c) a 100k-entry directory render benchmark.
2. **M2 — Core file ops:** selection, F5/F6/F7/F8 with dialogs, progress, error recovery.
3. **M3 — Command line & menus:** shell passthrough, Ctrl+O, F9 menus, sort modes, drive select, Info mode.
4. **M4 — Viewer & external editor:** F3 text/hex viewer, F4 external editor hook.
5. **M5 — Built-in editor & modern extras:** F4 built-in editor (minimal v1 floor per §4.6), quick filter, fuzzy jump, tabs, git info, user menu, find file, remaining panel modes (Brief/Tree/Quick View), help.

Each milestone ends in a working, demoable binary.

## 10. Project conventions

- **License:** MIT OR Apache-2.0 (dual, Rust-conventional).
- **MSRV policy:** latest stable minus 2; bumping MSRV is a minor-version change.
- **Config compatibility:** `config.toml` carries a schema `version` key; unknown keys warn rather than fail (see §6).
