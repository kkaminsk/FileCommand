# Change: clipboard-file-export

## Why

FileCommand cannot hand files to other applications. A terminal application cannot initiate an OS drag-and-drop (the terminal emulator owns the window and only forwards key/mouse events), so "drag a file from FileCommand into Explorer or Outlook" is impossible by construction. The clipboard is the substitute: put the selected entries on the clipboard as real file objects (`CF_HDROP`) and Ctrl+V in any Windows application pastes the files. Today Ctrl+C over the panels requests quit (the `quit-keys` change), which is both surprising to Windows users and squats on the one chord everyone expects to mean "copy".

## What Changes

- **Files to clipboard with Ctrl+C**: over the panels, Ctrl+C (and the fixed alias Ctrl+Ins) places the F5-scope entries — the selection set if non-empty, else the cursor entry, never `..` — on the OS clipboard as file objects (`CF_HDROP` + `Preferred DropEffect = COPY` on Windows), so a paste in Explorer, Outlook, Teams, etc. pastes the files.
- **Paths and names as text**: Ctrl+Shift+Ins copies one absolute path per line; a menu-only action copies one file name per line.
- **BREAKING (key map)**: Ctrl+C no longer requests quit anywhere; Esc keeps every quit/dismiss meaning from `quit-keys` unchanged. In the quit-confirmation dialog Ctrl+C no longer confirms. The built-in editor's Ctrl+C = Copy is unchanged.
- **Menus**: the Files pull-down gains a separated group `Copy to clipboard  Ctrl-C` / `Copy path(s)  Ctrl-Sh-Ins` / `Copy name(s)`; the Enter file-action menu gains `Send to clipboard` as its last entry.
- **Feedback** in the active panel's mini-status (`3 files copied to clipboard`, `Clipboard busy — try again`), cleared on the next key or after ~3 s.
- **Architecture**: the reducer emits `Effect::SetClipboard(ClipboardPayload)`; the TUI executes it synchronously behind a `Clipboard` trait (Windows: `clipboard-win`; elsewhere `arboard` text, with the file-list action falling back to paths-as-text). Core stays OS-free. Paths written to `CF_HDROP` are absolute with any `\\?\` / `\\?\UNC\` prefix stripped.

## Capabilities

### New Capabilities

- `clipboard-export`: payload kinds and selection scope; key bindings; Windows `CF_HDROP` format and path rules; non-Windows fallback; clipboard-busy retry; mini-status feedback; menu placement.

### Modified Capabilities

- `application-shell`: "Quit request keys and confirmation" — Ctrl+C removed from the quit triggers and from the dialog's confirm keys; Esc behaviour unchanged.
- `pulldown-menus`: "Menu contents" — Files menu gains the three clipboard items.
- `file-action-menu`: "Menu contents, ordering, and navigation", "Menu actions route to existing flows", and "No mutation without an intervening dialog" — `Send to clipboard` appended as a non-mutating entry.
- `help-and-about`: "Help topic pages" — the `Modern extras` page documents the clipboard bindings.

## Impact

- `crates/filecommand-core/src/update.rs` — `Command::CopyToClipboard(ClipboardPayloadKind)`, `Command::ClipboardResult`, `Effect::SetClipboard`, mini-status feedback state with `Tick` expiry (the reducer is key-agnostic; the Ctrl+C change is entirely in the TUI key map).
- `crates/filecommand-core/src/config.rs` — `Keys::clipboard_files` (default Ctrl+C) and `Keys::clipboard_paths` (default Ctrl+Shift+Ins), read as `key.clipboard_files` / `key.clipboard_paths` through the existing `parse_binding` path; `dialogs.rs` help text (`MODERN_EXTRAS`, and `CONFIGURATION`'s list of remappable keys).
- `crates/filecommand-core/src/menu.rs` — Files menu items (carrying their `MenuItem::shortcut` hints) and `MenuAction` variants.
- `crates/filecommand-tui/src/input/mod.rs` (+ `input/tests.rs`) — Ctrl+C / Ctrl+Ins / Ctrl+Shift+Ins mapping (matched ahead of the unguarded `Insert → ToggleSelectAtCursor` arm); the Ctrl+C → `RequestQuit`/`ConfirmQuit` arms removed in panels, viewer, menus, dialogs, and overlays, with modifier guards added where a bare `c` arm would otherwise catch Ctrl+C (progress-dialog cancel, Help cancel).
- `crates/filecommand-tui/src/clipboard.rs` (new) — `Clipboard` trait, Windows implementation (`clipboard-win`, gated `[target.'cfg(windows)'.dependencies]`), text-only implementation elsewhere, recording implementation for tests.
- `crates/filecommand-tui/src/app.rs` — `run_effects` arm for `SetClipboard`.
- Snapshot goldens for the Files menu and the action menu.
- Depends on the `quit-keys` and `enter-file-action-menu` changes (both implemented, not yet archived): this change's MODIFIED requirements are written against their delta text.
