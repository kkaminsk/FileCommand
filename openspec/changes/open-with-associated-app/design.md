# Design: open-with-associated-app

## Context

`FileActionMenuState::open(target, is_dir, executable, selection_scoped)` builds the entry list; `update.rs:2691-2701` routes Run to `Effect::RunShellCommand` (suspends the TUI, waits for a key) and View/Edit to the F3/F4 handlers. `Effect`s are executed in `app.rs`; failures come back as `Command`s that set `panel.last_error` (the pattern `EditorOpenFailed` uses). `clipboard-export` already strips `\\?\` before handing paths to Win32 (its "Long-path prefix is stripped" scenario under "Windows file-object payload"). `windows-sys` is only a transitive dependency today.

## Goals / Non-Goals

**Goals:**

- One keystroke pair (Enter, Enter) to open a document in its app.
- No terminal takeover.
- Clear failure text.
- Explorer for directories.

**Non-Goals:**

- Choosing the application ("Open with…").
- Waiting for the app and re-reading the panel.
- Associations configurable in FileCommand.
- Replacing Run (console programs still need the suspended path so their output is visible).

## Decisions

### D1: `ShellExecuteW` on Windows, detached spawn elsewhere

`ShellExecuteW(NULL, L"open", path, NULL, cwd, SW_SHOWNORMAL)` via `windows-sys`; a return value ≤ 32 is an error code: `SE_ERR_NOASSOC` (31) → `No application is associated with <name>`; `ERROR_FILE_NOT_FOUND` (2) → `<name> not found`; anything else → `Could not open <name> (error <code>)`. Non-Windows: `xdg-open` (Linux/BSD) or `open` (macOS) with stdio set to null and no wait; a spawn error maps to the same failure command. The TUI is never suspended, so a GUI app simply appears beside the terminal.

Alternative rejected: `cmd /C start "" "<path>"` through the existing shell path — it suspends the TUI, waits for a key, and `start`'s quoting rules are a known trap.

### D2: Entry order and default highlight

Non-executable file: `Open, View, Edit, Send to clipboard, Copy, Rename, Move, Delete` (Open highlighted). Executable: `Run, Open, View, …` (Run highlighted). Directory (right-click): `Open, Send to clipboard, Copy, Rename, Move, Delete`. Hotkey `O` is unique; `R` keeps resolving to Run when listed, else Rename.

### D3: Single-target, no selection scope, no re-read

Like Run, Open acts on the menu's target only, even when the target is selected. Because the launch is fire-and-forget, no panel re-read is scheduled; Ctrl+R remains the user's refresh.

### D4: Console-application associations

If a file type is associated with a console program (e.g. `.py` → `python.exe`), ShellExecute attaches it to our console and its output interleaves with the TUI. Documented in the README as the reason Run exists; not mitigated in this change.

## Risks / Trade-offs

- [New compile-time dependency on `windows-sys` (cfg(windows) only)] → Small, widely used, no runtime cost.
- [Default highlight moves from View to Open] → Muscle memory of "Enter, Enter = View" changes once; user-approved.

## Open Questions

- None.
