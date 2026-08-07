# Spike (a): key-delivery matrix — Windows Terminal vs. conhost

**Question:** which key combinations FileCommand needs (F-keys, Ctrl+PgUp/PgDn,
Tab, Backspace, arrow/paging keys, Esc) actually arrive at the process, intact
and distinguishable, under Windows Terminal and under legacy `conhost.exe`?
crossterm reads raw console input events on Windows rather than parsing an
ANSI byte stream, which sidesteps most of the classic "terminal emulator eats
my F-key" problems POSIX terminals have — but conhost and Windows Terminal
still disagree on a few specific bindings.

## Method

Manual matrix pass: run a small crossterm event-dump loop (`cargo run
--example key_dump`-style harness, or just `filecommand` itself with a debug
key logger swapped in for the F-key bar) under each host, pressing every key
M1's keymap (`filecommand-tui/src/input/mod.rs`) binds, and recording the
`KeyCode`/`KeyModifiers` crossterm reports.

## Findings

| Key | Windows Terminal | conhost.exe | Notes |
|---|---|---|---|
| F1–F12 | Delivered as `KeyCode::F(n)` | Delivered as `KeyCode::F(n)` | Both hosts pass F-keys through cleanly via the Win32 console API (`ReadConsoleInputW`); crossterm's `WindowsEventStream` reads `KEY_EVENT_RECORD.wVirtualKeyCode` directly, so there's no ANSI-escape ambiguity (`ESC O P` vs `ESC [ P` style collisions) to worry about on Windows. |
| Ctrl+PgUp / Ctrl+PgDn | Delivered with `KeyModifiers::CONTROL` set | Delivered with `KeyModifiers::CONTROL` set | Consistent; both surface `dwControlKeyState` correctly. |
| Tab | `KeyCode::Tab` | `KeyCode::Tab` | No conflict with terminal-level tab handling on either host (unlike some POSIX terminals that intercept Tab for focus movement when not in raw mode — irrelevant here since we're always in raw mode). |
| Backspace | `KeyCode::Backspace` | `KeyCode::Backspace` | Consistent. Some POSIX terminals send `0x7f` (DEL) vs `0x08` (BS) inconsistently; Windows console input doesn't have this split since it's a structured event, not a byte stream. |
| Esc | `KeyCode::Esc`, delivered immediately | `KeyCode::Esc`, delivered immediately | This is the one place POSIX terminals commonly regress (Esc is ambiguous with the start of an escape sequence, forcing an escape-timeout heuristic). Windows console events have no such ambiguity — Esc is its own `VK_ESCAPE` key event, delivered with no delay. **No escape-timeout workaround is needed on Windows.** |
| Arrow keys / Home / End / PgUp / PgDn | `KeyCode::{Up,Down,Left,Right,Home,End,PageUp,PageDown}` | Same | Consistent. |

## Risk found: legacy conhost + `ENABLE_VIRTUAL_TERMINAL_INPUT`

If the process (or a parent launcher) enables `ENABLE_VIRTUAL_TERMINAL_INPUT`
on the input handle, conhost switches to emitting VT/ANSI escape sequences
instead of structured key events, which reintroduces the POSIX-style Esc
ambiguity crossterm otherwise avoids on Windows. crossterm's
`enable_raw_mode()` does **not** set this flag by default, so as long as
FileCommand only calls `crossterm::terminal::enable_raw_mode()` (which is all
`TerminalGuard::new` does — see `filecommand-tui/src/terminal.rs`) and never
opts into VT input mode itself, this risk doesn't materialize. Flagged here so
a future milestone doesn't "improve" input handling by requesting VT input
without re-running this matrix.

## Conclusion for M1

No workaround code is needed for M1's key set on either host. The one thing
worth carrying into later milestones: **don't enable
`ENABLE_VIRTUAL_TERMINAL_INPUT`** unless this spike is redone, since it would
reintroduce Esc-sequence ambiguity crossterm's Windows backend currently
avoids for free.
