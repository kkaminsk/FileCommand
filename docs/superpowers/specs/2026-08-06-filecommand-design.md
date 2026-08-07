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
5. Fast startup (< 200 ms to first paint) and responsive UI on directories with 100k+ entries. First paint is decoupled from the first listing: the initial directory listing streams in after paint (a 100k-entry directory fills progressively), and git/drive info is always async and never gates paint. The first painted frame is the startup splash (§4.8) unless disabled, so first paint never waits on any I/O.

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
- **`theme`** — named 16-color-style theme model (role → color mapping: panel frame, directory text, file text, selected, cursor, key bar, dialogs, viewer). Ships with built-in `nc-classic` (default, matching the screenshots), plus `nc-mono` (black/white). User themes are TOML files. The concrete role list and the full `nc-classic` / `nc-mono` role→color tables are normative in §4.11, which also defines the color-depth policy (ANSI-16 named colors required for every role; per-role truecolor optional).
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
   - **Mini-status line** at the panel bottom (inside the border): current entry's name, size, date, time; when files are selected, shows `N files selected, X bytes` instead. Selected directories contribute 0 bytes to the total (classic NC behavior — directories are not sized in v1). While a directory listing is still streaming in, the mini-status line instead shows `Reading… 12,345` (running entry count, updating as chunks arrive); it reverts to the normal display when the read completes (§4.10).
   - **Tab strip**: when a panel has more than one tab, a single compact row appears above that panel (the panel shrinks by one row); hidden with one tab (preserves classic look). Blue background; each tab renders as ` n:NAME ` where `n` is the tab number and `NAME` is the directory basename, uppercased and truncated to fit. Active tab: black on cyan; inactive tabs: cyan on blue. Overflow: when tabs exceed the panel width, labels shrink stepwise to ` n:NAM… ` and finally ` n `; if still overflowing, the strip scrolls to keep the active tab visible, with `◄` / `►` overflow markers (cyan on blue) at the strip's ends.
   - A clock (`h:mm a` style, black on cyan) drawn over the right end of the right panel's top border, top-right corner of the screen (as in the screenshots). Hidden while the F9 menu bar is open.
   - There is **no persistent menu bar**. In the normal state the top screen row is occupied by the panels' top borders and the clock. Pressing F9 temporarily overlays a single menu-bar row (full width, black on cyan) across the top row: `  Left     Files     Commands     Options     Right  ` — hiding the clock while open (see §4.3).
2. **Command line** — shell prompt with the active panel's path (`C:\NORTON>_`). Printable keys typed while a panel is focused go to the command line, exactly like NC. Enter runs the command via the shell (TUI suspends, output shown, "press any key" to return). Ctrl+Enter pastes the current filename to the command line; Ctrl+] pastes the current path. History with Up/Down when the command line is non-empty — this means the panel cursor cannot be moved mid-composition; Esc clears the command line to return Up/Down to the panel. Alt+F8 opens a history dialog.
3. **F-key bar** — one row, ten slots: `1Help 2Menu 3View 4Edit 5Copy 6RenMov 7Mkdir 8Delete 9PullDn 10Quit`. Holding Ctrl or Alt relabels the bar live to the modifier's meanings (e.g. Ctrl: `3Name 4Exten 5Time 6Size 7Unsort…`), as seen in the screenshots. **Feasibility gate:** live relabeling requires standalone modifier press/release events. Crossterm's Windows event parser explicitly drops bare `VK_SHIFT`/`VK_CONTROL`/`VK_MENU` console records (confirmed against crossterm's current source; unchanged since at least 2022) — modifier state only ever arrives attached to another key's event. This holds regardless of the hosting terminal: Windows Terminal added Kitty Keyboard Protocol support in February–March 2026 (PR [microsoft/terminal#19817](https://github.com/microsoft/terminal/pull/19817), shipped in Preview v1.25.622.0), but crossterm's Windows backend still hardcodes its keyboard-enhancement-flags path to fail unconditionally on Windows (crossterm issue [#1022](https://github.com/crossterm-rs/crossterm/issues/1022)), so it cannot exploit that terminal capability yet. This feature is best-effort and gated on an M1 spike verifying whether bare modifier events are obtainable some other way (a crossterm release that unlocks KKP on Windows, or a direct `ReadConsoleInput` call bypassing crossterm's parser). Fallback if unavailable: relabeling is dropped and the Ctrl/Alt variants are documented in F1 Help instead.

### 4.2 Panel display modes

- **Brief** — three columns of names only.
- **Full** (default) — Name | Size | Date | Time.
- **Info** — replaces the panel with system/drive/directory info (version banner, memory, drive total/free bytes, volume label, serial, file/dir counts), mirroring the Info panel screenshot. Layout mirrors that screenshot: stacked single-line-framed boxes; label text cyan, numeric values bright-yellow, the version banner (the identity lines of §4.8, shared verbatim with the splash and About dialog) bright-white. Async values (drive totals/free, file/dir counts) render as `…` until their background query completes, then are replaced in place (§4.10).
- **Tree** — lazily-expanded directory tree of the current drive (directories are read on expand, not by scanning the whole drive up front). Moving the cursor updates the *opposite* panel to list the highlighted directory; Enter returns *this* panel to its previous list mode at the chosen directory. Visual layout: the column-header row reads `Tree`; the first body row is the drive root (`C:\`, bright-white); descendants are drawn with single-line branch glyphs (`│  `, `├─`, `└─`, cyan) indenting one level per depth, directory names in bright-white UPPERCASE. The cursor is the standard inverse bar; the mini-status line shows the highlighted directory's full path. Directories not yet expanded simply show no children (lazy read on expand); no `+`/`-` expander glyphs in v1.
- **Quick View** — shows a preview (text head) of the file under the opposite panel's cursor. Visual layout: the panel's top-border title reads `Quick view` (inverse when active); the body renders the file head exactly like viewer text mode (wrap on, lossy UTF-8, no viewer controls), in the viewer text style. The mini-status shows the previewed file's name and size. When the opposite cursor is on a directory, the body shows a centered `▶SUB-DIR◀` and no preview. Binary content is shown with lossy replacement characters (no hex mode in Quick View).

### 4.3 Pull-down menus (F9)

Five menus: **Left**, **Files**, **Commands**, **Options**, **Right**. Navigation with arrows/Enter/Esc; hotkey letters highlighted.

- **Left/Right** — display mode, sort mode, filter, re-read, drive select, new/close tab (mirror of each other).
- **Files** — View, Edit, Copy, Rename/Move, Make directory, Delete, Attributes (read-only/hidden/archive toggle dialog), Select group / Deselect group / Invert (also `+`/`-`/`*` keys), Quit.
- **Commands** — Find file, History, Swap panels, Panels on/off, Compare directories, Fuzzy jump, Menu file edit (user menu).
- **Options** — Configuration (confirmations, panel options, show hidden/system files — default off, matching NC; hidden entries render dimmed when shown), Themes, Editor selection, Save setup.

**Visuals:** F9 overlays the menu-bar row described in §4.1 (black on cyan, full width, replacing the top screen row and clock). The active menu title is highlighted white on black; menu hotkey letters are bright-yellow. The open pull-down is a single-line-framed box (black frame, black text on cyan) hanging below its title; the selected item is white on black; disabled items are white (grey) on cyan; separator rows use `─`. Esc closes the pull-down, then the bar, restoring the top row. Left/Right arrows move between menus with the pull-down staying open.

### 4.4 Dialogs

NC-style modal boxes centered over the panels: **primary dialogs** are black text on a cyan background with a black double-line frame and the title set into the top border (per the Help screenshot); **nested/secondary dialogs** use the alternate grey style (black on white, black single-line frame); **error dialogs** (Retry/Skip/Abort, panel read errors) are bright-white on red with a bright-white frame. Input fields render with the classic bracket-and-dots styling (`[.......]`, black on cyan, dots filling unused width; the text cursor is the terminal cursor). Buttons are highlighted labels navigated with Tab/arrows: unfocused black on white, focused/default black on bright-yellow; activated with Enter, dismissed with Esc. Standard dialogs: message, confirm (Yes/No), input (mkdir, rename target, copy destination pre-filled with opposite panel path), overwrite conflict, error Retry/Skip/Abort, progress (file counts, byte progress bar, current path, Cancel). The progress dialog's byte bar is drawn with `█` block glyphs, blue on the cyan dialog body, with the empty remainder as `░` (black on cyan).

### 4.5 Viewer (F3)

Built-in read-only viewer with two modes, switchable via F4-in-viewer (label toggles `Hex`/`ASCII` as in the screenshots):

- **Text mode** — UTF-8 with lossy fallback; wrap/unwrap toggle (F2); search (F7); Col/offset, size, and percent indicators in the header.
- **Hex mode** — classic offset | hex bytes | ASCII gutter layout.

Streams from disk; must open multi-GB files instantly. The viewer memory-maps or chunk-reads and builds no full line index: the percent indicator is byte-offset-based; backward navigation scans backward from the current offset for line starts, with a max-line-length cap (e.g. 64 KB) after which lines are hard-split; search streams with overlap at chunk boundaries; hex mode is pure offset math. Viewer F-key bar: `1Help 2Unwrap 4Hex 7Search 10Quit`.

### 4.6 Editor (F4)

Built-in minimal editor for quick edits: insert/overwrite, cut/copy/paste line-based selection, search/replace, undo, save (F2), UTF-8 + CRLF/LF preservation, "modified" indicator and save-on-exit prompt. `config.toml` may set an external editor command; when set, F4 suspends the TUI and launches it. The built-in editor targets files < 10 MB (larger files open in the viewer with a notice).

To contain scope, the v1 built-in editor floor is explicitly minimal: single-level undo batch, line-based selection only, no regex in search/replace. The external-editor hook ships first (M4) so F4 is useful before the built-in editor lands (M5).

**Editor chrome.** Full-screen, replacing the panels, mirroring the viewer's frame-less layout (§4.5):

- **Header row** (top screen row, black on cyan, full width): left `Edit: C:\path\file.txt`, with ` *` appended after the path whenever there are unsaved changes (the modified indicator); center `Line 12/440   Col 8`; right the file size in bytes and, when overwrite mode is active, `Ovr`.
- **Body**: file text in the editor text style (white on blue, §4.11); the caret is the terminal cursor; line-based selection renders as inverse rows.
- **F-key bar** (bottom row, same styling as the main bar): `1Help 2Save 3Mark 4Replac 5 6 7Search 8 9 10Quit`. Unused slots render the number with an empty label block, exactly as the viewer screenshots do. F2 saves in place, F3 toggles the line-selection anchor (Mark), F4 opens search-and-replace, F7 search, F10 quit (with the save-on-exit prompt when modified).

### 4.7 Modern extras

- **Quick filter (Ctrl+P)** — inline input in the mini-status line; panel narrows to substring matches as you type; Esc clears. Note: Ctrl+P intentionally deviates from classic NC (where it toggles the inactive panel — a feature not implemented in v1); the binding is overridable in `config.toml`.
- **Type-ahead jump** — Alt+letter starts quick-search mode and moves the cursor to the first match; while active, *plain* printable keys extend the search pattern (shown in the mini-status line), Backspace shortens it, and Esc/movement keys exit, returning printable keys to the command line (this resolves the conflict with command-line typing in §4.1).
- **Fuzzy jump (Ctrl+J)** — dialog with a fuzzy-matched, frecency-ranked list of previously visited directories; Enter navigates the active panel there. Directory history persists across sessions.
- **Panel tabs (Ctrl+T new, Ctrl+W close, Alt+1..9 switch)** — independent directory+state per tab.
- **Git info** — inside a repository, the active panel's top border shows ` (branch-name)` after the path, and a one-cell status marker column (`M`/`?`/`+`) appears before file names. Silent and absent outside repositories or on timeout.
- **Themes** — `theme = "nc-classic"` in config; theme TOML files in the config directory; Options → Themes switches at runtime.

### 4.8 Startup splash

On launch, the very first painted frame is an authentic NC-style splash: a solid blue backdrop with a centered double-line box containing the product name, version, and copyright-style lines. It is not ASCII art — plain centered text in a box, in the spirit of NC's About dialog. Because it renders from static data, it satisfies the < 200 ms first-paint goal (§2) unconditionally; panel initialization, the first directory listing, and git/drive queries all proceed behind it.

**Behavior:**

- **Timing:** the splash paints as frame 1 and holds for a minimum of **800 ms**, then is replaced by the panels — even if the initial listing is still streaming (the panels then fill progressively per §4.10).
- **Dismissal:** any key press dismisses the splash immediately (the minimum hold does not delay an explicit key). The dismissing key event is consumed — never forwarded to the command line or panels.
- **Disabling:** `splash = false` in `config.toml` general options, or the `--nosplash` CLI flag (flag overrides config). When disabled, frame 1 is the panels.
- **Resize:** the box re-centers on terminal resize. If the terminal is below the 80×24 minimum at startup, the splash is skipped in favor of the "terminal too small" placeholder (§4); shrinking below minimum mid-splash replaces it with the placeholder, and the splash does not return.
- **Rendering:** the terminal cursor is hidden during the splash. Colors come from the `splash.*` theme roles (§4.11); under `nc-mono` it renders white on black. The version string is the crate version; the identity lines (name, version, copyright, tribute) are shared verbatim with the About dialog (§4.9) and the Info-panel version banner (§4.2) — single source of truth.

**Mockup** (80×24, `nc-classic`; the box is 48 columns wide, horizontally and vertically centered — at 80×24 it occupies rows 8–16, columns 17–64; all surrounding rows are solid blue):

```
                ╔══════════════════════════════════════════════╗
                ║                                              ║
                ║                 FileCommand                  ║
                ║                Version 0.1.0                 ║
                ║                                              ║
                ║  Copyright (C) 2026 The FileCommand Authors  ║
                ║  Inspired by the Norton Commander, 1986-1998 ║
                ║                                              ║
                ╚══════════════════════════════════════════════╝
```

Frame: cyan on blue. `FileCommand`: bright-white. Version line: white. Copyright/tribute lines: cyan.

### 4.9 Help (F1) and About

F1 opens the Help window, laid out per the NC 5 Help screenshot: a centered window (approximately 62×19 at 80×24; scales with terminal size, capped near that proportion) in the primary dialog style — cyan background, black double-line frame, the title `Help` set into the top border on a small black-framed tab.

- **Header block:** three centered black-on-cyan lines mirroring NC's: the identity lines of §4.8 (name + version, copyright, tribute) — the same strings as the splash.
- **Topic list:** below the header, a scrollable list of help topics; the first entry is **`About FileCommand`**, highlighted (white on black) as the initial cursor position. Remaining v1 topics: `Keyboard reference`, `Panels and display modes`, `File operations`, `Menus`, `Viewer`, `Editor`, `Command line`, `Modern extras`, `Configuration`. Scroll arrows `↑` / `↓` render on the right border when the list overflows.
- **Buttons:** `Help` (default, black on bright-yellow — opens the highlighted topic, same as Enter) and `Cancel` (black on white — closes, same as Esc).
- **Topic pages** replace the list within the same window; Esc returns to the list. Topic content is static text compiled into the binary. The Ctrl/Alt F-key-bar variants are documented under `Keyboard reference` (this is the documented fallback of §4.1's relabeling feasibility gate).
- **About dialog:** Enter on `About FileCommand` opens a secondary (grey-style, §4.4) dialog, centered, roughly 52×10: the identity lines, plus `License: MIT OR Apache-2.0` (§10) and the repository URL, with a single `OK` button. This is the About content the splash mirrors.

### 4.10 Loading and async affordances

FileCommand never uses spinners or animation glyphs; all loading feedback is period-authentic static text updated in place (the same idiom as NC's copy-dialog counters).

- **Streaming directory listings:** the panel body renders entries as chunks arrive, inserted in sorted position; the cursor holds on the first row until the user moves it. While incomplete, the mini-status shows `Reading… N` (running count, §4.1). No overlay, no dimming — a partially filled panel is fully interactive.
- **Git info:** intentionally indicator-free (matching §4.7's "silent and absent" rule). The ` (branch)` suffix and the one-cell status-marker column appear together, in a single reflow, when the background query resolves; nothing is reserved or shown while pending.
- **Info panel:** async values (drive totals/free, counts, volume label/serial) show `…` until resolved (§4.2).
- **Drive-select dialog (Alt+F1/F2):** drive letters appear immediately; the volume-label column is blank per drive until its lazy fetch completes, then fills in. The dialog never blocks on media/network probing (§5 notes).
- **Quick View / viewer:** open instantly at any file size (§4.5); no loading state needed.

### 4.11 Color palette and rendering policy

#### Color-depth policy

Theme files specify colors by **ANSI-16 name**: `black, red, green, yellow, blue, magenta, cyan, white` and their `bright-` variants (`white` is the classic grey; `bright-black` is dark grey). Rendering emits standard 16-color attributes, so the host terminal's palette remaps everything naturally — a VGA-style Windows Terminal scheme reproduces the DOS look exactly, and user terminal themes are respected. Bright *backgrounds* are permitted (classic DOS achieved these by disabling blink; modern terminals support them directly). A theme MAY additionally give any role a truecolor `#RRGGBB` value, used only when the terminal supports truecolor; the named ANSI-16 value remains mandatory for every role as the fallback. There is no 256-color indexed support.

#### Iconography

Rendering uses ASCII plus the CP437-heritage box-drawing and geometric glyphs only (`═ ║ ╔ ╗ ╚ ╝ ─ │ ┌ ┐ └ ┘ ├ ┤ ▶ ◀ ↑ ↓ ◄ ► █ ░ …`), all single-cell in `unicode-width`. **No Nerd Font glyphs, no emoji, no file-type icons** — file-type differentiation is by color and case, exactly as in NC. This guarantees column alignment in any monospace font.

#### `nc-classic` (default; derived from the reference screenshots)

| Role | Fg | Bg |
|---|---|---|
| `screen.backdrop` | — | blue |
| `panel.frame` (double lines, column separators) | cyan | blue |
| `panel.title.active` (path in top border) | black | cyan |
| `panel.title.inactive` | cyan | blue |
| `panel.header` (column labels + sort arrow) | bright-yellow | blue |
| `panel.file` | cyan | blue |
| `panel.directory` (and `▶UP--DIR◀` / `▶SUB-DIR◀`) | bright-white | blue |
| `panel.hidden` (dimmed hidden/system entries) | bright-black | blue |
| `panel.selected` | bright-yellow | blue |
| `panel.cursor` (full-width inverse bar) | black | cyan |
| `panel.cursor.selected` | bright-yellow | cyan |
| `panel.git.modified` | bright-yellow | blue |
| `panel.git.untracked` | bright-cyan | blue |
| `panel.git.staged` | bright-green | blue |
| `panel.ministatus` (incl. `Reading… N`, quick-filter/search input) | cyan | blue |
| `tab.active` | black | cyan |
| `tab.inactive` | cyan | blue |
| `clock` | black | cyan |
| `commandline` | white | black |
| `keybar.number` | white | black |
| `keybar.label` | black | cyan |
| `menubar` / `menu.body` | black | cyan |
| `menu.highlight` | white | black |
| `menu.hotkey` | bright-yellow | cyan |
| `menu.disabled` | white | cyan |
| `dialog.primary` (body, frame, title) | black | cyan |
| `dialog.secondary` (grey style) | black | white |
| `dialog.error` | bright-white | red |
| `dialog.input` (bracket-and-dots field) | black | cyan |
| `button.normal` | black | white |
| `button.focused` | black | bright-yellow |
| `dialog.gauge.filled` (`█`) | blue | cyan |
| `dialog.gauge.empty` (`░`) | black | cyan |
| `viewer.header` / `editor.header` | black | cyan |
| `viewer.text` / `editor.text` (text + hex body) | white | blue |
| `viewer.match` (search hit) | black | cyan |
| `info.label` | cyan | blue |
| `info.value` | bright-yellow | blue |
| `info.banner` | bright-white | blue |
| `splash.frame` | cyan | blue |
| `splash.title` | bright-white | blue |
| `splash.version` | white | blue |
| `splash.text` | cyan | blue |
| `screen.placeholder` ("terminal too small") | white | blue |

#### `nc-mono`

Everything white on black; frames, `…`, and gauges white; directories and selected entries bright-white; every role above that is inverse in `nc-classic` (cursor, active title, key-bar labels, clock, active tab, menus, dialogs, viewer/editor headers, buttons) becomes black on white; error dialogs black on white with a `!` prefix in the title; git markers plain white. No color carries meaning that isn't also carried by case, position, or inversion.

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

- `config.toml` — general options (confirmations, panel defaults, editor command, theme name, clock format, splash on/off), keybinding overrides.
- `themes/*.toml` — user themes.
- `usermenu.toml` — F2 user menu entries (label + command).
- `history.json` — command history, directory frecency data (written atomically).

Command-line flags: `--nosplash` skips the startup splash (§4.8), overriding config. Theme TOML files map the role names of §4.11 to ANSI-16 color names, with optional `#RRGGBB` truecolor overrides per role.

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
- **TUI snapshot tests:** ratatui `TestBackend` renders key screens (Full panel, Brief, Info, dialogs, viewer text/hex, F-key bar modifier variants, splash (§4.8), Help window and About dialog, editor chrome, tab strip, Tree and Quick View modes, streaming mini-status) compared against committed snapshots via the `insta` crate (ratatui's own documented recipe for `TestBackend`-based snapshot testing). The TUI takes an injected time source (`Clock` trait) so the on-screen clock is pinnable; snapshot tests pin time, terminal size, and locale, and fixture directories use fixed timestamps.
- **Integration smoke test:** scripted event sequence (navigate, select, copy, verify result on disk) run in CI.
- **CI platforms:** the full suite runs on `windows-latest` (the only supported platform); Linux CI builds and runs core tests only, as a compile guard for the cross-platform claim.
- **Manual test checklist:** Windows Terminal rendering (colors, box-drawing glyphs), long-path (> 260 chars) operations, a 100k-entry directory, a multi-GB file in the viewer.

## 9. Milestones

1. **M1 — Shell:** workspace scaffolding, event loop, theme system, two panels rendering a real directory (Full mode), navigation, Tab, F10 quit, startup splash (§4.8) with `--nosplash`. Includes three de-risking spikes: (a) key-delivery matrix in Windows Terminal and conhost, (b) standalone modifier press/release detection (gates F-key bar live relabeling, §4.1), (c) a 100k-entry directory render benchmark.
2. **M2 — Core file ops:** selection, F5/F6/F7/F8 with dialogs, progress, error recovery.
3. **M3 — Command line & menus:** shell passthrough, Ctrl+O, F9 menus, sort modes, drive select, Info mode.
4. **M4 — Viewer & external editor:** F3 text/hex viewer, F4 external editor hook.
5. **M5 — Built-in editor & modern extras:** F4 built-in editor (minimal v1 floor per §4.6), quick filter, fuzzy jump, tabs, git info, user menu, find file, remaining panel modes (Brief/Tree/Quick View), help window + About dialog (§4.9).

Each milestone ends in a working, demoable binary.

## 10. Project conventions

- **License:** MIT OR Apache-2.0 (dual, Rust-conventional).
- **MSRV policy:** latest stable minus 2; bumping MSRV is a minor-version change.
- **Config compatibility:** `config.toml` carries a schema `version` key; unknown keys warn rather than fail (see §6).
