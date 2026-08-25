# Design: clipboard-file-export

## Context

Copy/Move already resolve their scope through `active_selection_sources` (selection set, else cursor entry, never the parent pseudo-entry) and run as jobs; nothing in core knows about the OS clipboard. The `Effect` enum is the seam for OS interaction and already has a synchronous-on-the-UI-thread precedent (`EnumerateDrives`). The `quit-keys` change (implemented, unarchived) made Ctrl+C a quit trigger everywhere except the built-in editor and made Ctrl+C confirm inside the quit dialog; the user has reversed the Ctrl+C half of that decision — Esc keeps every meaning it has today. Windows Terminal reserves Ctrl+Shift+C for its own copy; Ctrl+Alt+C is AltGr+C on many keyboard layouts; Ctrl+Ins / Ctrl+Shift+Ins are the classic Windows/Far chords and every host delivers them. Pull-down items already render a right-aligned shortcut hint (`MenuItem::shortcut`), so the new items are discoverable without any menu-rendering change.

## Goals / Non-Goals

**Goals:**

- Pasting into Explorer/Outlook after Ctrl+C in FileCommand pastes the actual files.
- Paths and names as text for chats, editors, and shells.
- The same scope rule as F5, so the clipboard actions never surprise a user who just selected files.
- Every clipboard action reachable from the keyboard, the Files pull-down, and the file-action menu.
- Clear feedback; core stays OS-free and unit-testable.

**Non-Goals:**

- OLE drag initiation (impossible from a terminal).
- Cut (`DropEffect = MOVE`) and paste-into-FileCommand (Ctrl+X / Ctrl+V) — v2 candidates.
- Clipboard history; images or rich content; copying from the viewer or editor.

## Decisions

### D1: Ctrl+C means copy-to-clipboard, and only that

Per the user's decision, Ctrl+C is unbound from quit in the panels, viewer, menus, and dialogs, and from "confirm" in the quit dialog. Esc remains the quit trigger over the panels and the universal dismisser elsewhere, exactly as `quit-keys` specified. Ctrl+Ins is a fixed alias (not rebindable) so the Windows/Far habit also works; `key.clipboard_files = "ctrl+k"` in `config.toml` rebinds the primary chord through the existing flat `key.<name>` binding syntax (`parse_binding`). The reducer never sees key codes — the whole change is in the TUI key map, where the bare-`c` arms of the progress dialog and Help gain a no-modifier guard so a Ctrl+C falling through no longer cancels a job or closes Help. Rejected: keeping Ctrl+C as quit and using Ctrl+Ins alone — the user chose Explorer muscle memory.

### D2: Three payload kinds, one command each

`ClipboardPayloadKind::{Files, Paths, Names}`. Files → `CF_HDROP` (plus `Preferred DropEffect = COPY`) on Windows; Paths/Names → Unicode text, one item per line, no trailing separator after the last. Names is menu-only to keep the chord count small (Ctrl+Shift+Ins = Paths). Scope is `active_selection_sources` — identical to F5/F6. Rejected: a single action with a mode dialog — one more keystroke on every use for a rare choice.

### D3: Clipboard is an `Effect` executed synchronously on the UI thread behind a trait

`Effect::SetClipboard(ClipboardPayload { kind, items: Vec<PathBuf> })` is executed in `run_effects` via a `Clipboard` trait (`set_files`, `set_text`). `OpenClipboard` binds to the calling thread and set-and-close is sub-millisecond, so no worker thread is needed; the result returns as `Command::ClipboardResult(Result<usize, String>)`. The Windows implementation uses `clipboard-win` 5.x (`formats::FileList`, `register_format("Preferred DropEffect")` with `RawData` for the 4-byte `DROPEFFECT_COPY`, `Clipboard::new_attempts(5)` with ~20 ms back-off for the "clipboard busy" case). Rejected: `arboard` for files (no file-list support on Windows); the raw `windows` crate (hand-rolled `DROPFILES`).

### D4: Paths are absolute and prefix-stripped

Items are `cwd.join(name)`; any `\\?\` prefix is removed and `\\?\UNC\server\share` is rewritten to `\\server\share` before the payload is written — Explorer's `DragQueryFileW` rejects the prefix. Paths over the legacy limit may still fail on paste in Explorer without the long-path policy; documented, not worked around.

### D5: Non-Windows is best-effort text

`arboard` for Paths/Names; Files falls back to the Paths text and the feedback says `Paths copied (file objects unsupported here)`. `text/uri-list` target negotiation is out of scope.

### D6: Feedback in the active panel's mini-status

There is no global status line. The mini-status shows the result until the next key press or ~3 s (`Tick`), then reverts to the selection summary / cursor-entry display. Failure after retries: `Clipboard busy — try again` in the error colour role.

### D7: Menu placement

Files pull-down: a separated three-item group after Delete (14 items + 4 separators = 18 content rows, 20 framed, 21 with the menu-bar row — still fits 24 lines; a submenu mechanism would cost more than three rows). Shortcut hints follow the existing hyphenated style (`Ctrl-C`, `Ctrl-Sh-Ins`, like `Ctrl-L`). Action menu: one item, last, labelled `Send to clipboard` (Explorer vocabulary; avoids the `C` hotkey clash with Copy). Paths/Names stay in the pull-down.

## Risks / Trade-offs

- [Users with `quit-keys` muscle memory press Ctrl+C to quit] → the first press copies and the mini-status says so; Esc and F10 still quit. Accepted per user decision.
- [Clipboard held by another process] → bounded retry, then an honest message.
- [Ctrl+Shift+Ins delivery in some Unix emulators] → rebindable; Paths is also in the menu.
- [Deltas target unarchived `quit-keys` / `enter-file-action-menu` text] → written against that text; archive/sync order is noted for the maintainer.

## Open Questions

- None blocking. v2 candidates: Ctrl+X (`DropEffect = MOVE`) and Ctrl+V reading `CF_HDROP` into the normal Copy/Move dialog.
