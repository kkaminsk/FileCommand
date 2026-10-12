# Change: open-with-associated-app

## Why

Enter on a document opens the file-action menu, whose entries are View, Edit, Send to clipboard, Copy, Rename, Move, Delete — and Run for executables only. There is no way to open a `.pdf`, `.png`, `.docx`, or `.xlsx` in the application Windows associates with it; View shows bytes and Edit opens a text editor. For a Windows-first file manager this is the most-used missing action. Norton Commander covered the same need through its extension file; the modern equivalent is `ShellExecute` with the `open` verb.

## What Changes

- A new **Open** entry in the file-action menu: first and highlighted for non-executable files (so Enter, Enter opens a document); for executables Run stays first and Open is second; for directory targets (right-click menu) Open is first and opens the directory in the system file browser. Hotkey `O`.
- Open launches the target through the platform's file association **without suspending the TUI** and without waiting: `ShellExecuteW(open)` on Windows, `xdg-open`/`open` detached elsewhere. The panel stays interactive.
- Failure (no association, launch error) is reported on the active panel's status line (`last_error`), e.g. `No application is associated with report.xyz`.
- Open is always single-target (like Run) and never touches the selection. Run, View, Edit, and every other entry are unchanged.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `file-action-menu`: "Menu contents, ordering, and navigation" (Open first, hotkey `O`), "Menu actions route to existing flows" (Open routing), "Directory targets and selection-scoped invocation" (Open first for directories), plus a new requirement "Open with the associated application" (launch semantics, non-blocking, failure reporting).

## Impact

- `crates/filecommand-core/src/dialogs.rs` — `FileActionMenuEntry::Open`, ordering, label/hotkey; `update.rs` — `Command::FileActionMenuConfirm` routing to `Effect::OpenWithAssociation{path, cwd}`; `Command::OpenWithAssociationFailed{message}` → `last_error`.
- `crates/filecommand-tui/src/app.rs` — effect handler; new `cfg(windows)` dependency `windows-sys` (features `Win32_UI_Shell`, `Win32_Foundation`, `Win32_UI_WindowsAndMessaging`) in `crates/filecommand-tui/Cargo.toml`; non-Windows `std::process::Command` detached spawn.
- Paths handed to the shell API are stripped of the `\\?\` verbatim prefix the same way `clipboard-export` does (its "Long-path prefix is stripped" scenario).
- Tests: menu contents/hotkeys, routing effect, failure message; snapshot of the menu with Open first.
- Non-breaking for keys; **changes the default highlighted entry** from View to Open for non-executables (user decision).
