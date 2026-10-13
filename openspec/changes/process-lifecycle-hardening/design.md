# Design: process-lifecycle-hardening

## Context

Verified on `build/open-proposals-2026-10` (`1bada6f`).

- **Suspended child runs.**
  - `Effect::RunShellCommand` is emitted by the file-action menu's Run entry (`update.rs:2875`) and by F2 user-menu entries (`update.rs:3557`). It reaches `run_shell_command` (`app.rs:1041-1056`), which calls `guard.suspend()`, then `std::process::Command::new(..).status()` with inherited stdio, then `wait_for_key()`, then `guard.resume()`.
  - `Effect::RunExternalEditor` reaches `run_external_editor` and `spawn_editor_and_wait` (`app.rs:1147-1176`) with the same shape.
  - `suspend` disables raw mode, which on Windows restores `ENABLE_PROCESSED_INPUT`, so Ctrl+C becomes a console control event again.
  - No code in the workspace calls `SetConsoleCtrlHandler`, installs a signal handler, or depends on a signal crate. The default handler terminates FileCommand along with the child.
  - `wait_for_key` re-enables raw mode for its read, so a Ctrl+C at the press-any-key prompt is an ordinary key and is not affected.
- **Quit.**
  - `ConfirmQuit` pushes `Effect::CancelJob` only when the phase is `FileOpRunning`, then `Effect::Quit` (`update.rs:1299-1309`).
  - `run_effects` sets `handle.cancel` (`app.rs:850-854`) and returns `quit = true`, and `run` returns `Ok(())` at once (`app.rs:391-394`, and the same `if quit { return Ok(()) }` after every `apply`).
  - `JobHandle { cancel, reply_tx }` (`worker.rs:216-219`) keeps no `JoinHandle`, and `spawn_job` (`worker.rs:408-421`) drops it. When `main` returns, the process exits and kills the job thread wherever it is.
  - The cancel cleanup in `copy_chunked_impl` (`fs_ops/fs.rs:263-268`: drop the destination, remove the file, return a cancelled error) runs only if the thread reaches the next chunk check first.
- **Waiting on a prompt.**
  - `ChannelSink::conflict` and `ChannelSink::error` (`worker.rs:234-254`) send `JobConflict`/`JobError` and then block in `self.reply_rx.recv()`, so the cancel flag is not observed.
  - On `Err` (the sender was dropped) they fall back to `ConflictChoice::Skip` and `ErrorChoice::Abort` respectively, so a lost conflict answer silently becomes Skip.
- **Input on the dialogs.** `map_file_op_running_key` (`input/mod.rs:800-835`):
  - On the conflict dialog with no rename field, it maps only O, W, S, A, and R, so Esc does nothing.
  - With the rename field open, Esc closes the field (`FileOpCancel`).
  - On the error dialog, it already maps Esc (and B) to `ErrorChoice::Abort`. The operation-dialogs spec does not say so.
- **The panic hook.** `install_panic_hook_with` (`terminal.rs:219-225`) installs one process-wide hook that runs `restore()` and then the previous hook for every thread.
  - None of the `spawn_*` bodies in `worker.rs` use `catch_unwind`.
  - `spawn_listing` sends `ListingComplete` or `ListingFailed` only on a normal return, and `spawn_job` relies on `run_job` calling `sink.done`.
  - A panic inside `git_info::query` is already absorbed: it runs on `race_with_timeout`'s inner thread, whose dropped sender makes `recv_timeout` return at once, giving `GitInfo::none()`. The hook still restores the terminal in that case.
  - No code path that actually panics was found in a worker. The audit rates the trigger as SUSPECTED.

## Goals / Non-Goals

**Goals:**

- Ctrl+C typed during a suspended child run reaches only the child.
- Quit lets a running job finish its cancellation cleanup, within a bound.
- The conflict dialog can cancel the job, and a cancel request is honored while a job waits on any prompt.
- A worker panic never touches the terminal and never leaves a dialog or `Reading…` indicator stuck.

**Non-Goals:**

- Changing how the job engine writes files: temporary file plus rename, `sync_all`, and verified moves are `durable-file-copy`'s. This change only gives the existing cancel cleanup time to run.
- A Ctrl+C or Ctrl+Break policy for the TUI itself. In raw mode, Ctrl+C is a key; its meaning is defined by application-shell "Quit request keys and confirmation" and clipboard-export.
- Cancelling find-file walks or viewer searches. That is P-10, in `listing-request-guard`.
- Restoring the R-03 orphaned-job case. `resize-preserves-session-state` removes it; this change only makes the quit path robust whatever the core phase.

## Decisions

### D1: Windows uses a console control handler gated by a "child running" flag

- At startup, `run` calls `SetConsoleCtrlHandler(Some(ctrl_handler), TRUE)` once.
- `ctrl_handler` returns `TRUE` (handled, so FileCommand keeps running) for `CTRL_C_EVENT` and `CTRL_BREAK_EVENT` while a process-wide `static CHILD_RUNNING: AtomicBool` is set. In every other case it returns `FALSE`, so default processing still ends the process on close, logoff, or shutdown, and on Ctrl+C or Ctrl+Break outside child runs.
- `run_shell_command` and `run_external_editor` set the flag through an RAII `ChildRunGuard`. The flag is set before `guard.suspend()` and held until `guard.resume()` has re-entered raw mode, so the press-any-key prompt and the moments between steps are covered too.
- The child is unaffected. Handler routines are per-process and are not inherited, so the child receives `CTRL_C_EVENT` and applies its own or the default handling.
- This needs the `windows-sys` feature `Win32_System_Console`.

*Alternative considered:* `SetConsoleCtrlHandler(None, TRUE)` around the spawn. This was rejected because the "ignore Ctrl+C" attribute it sets is inherited by child processes, so `ping -t` itself could no longer be stopped. The audit notes the same.

*Alternative considered:* spawn the child with `CREATE_NEW_PROCESS_GROUP`. This was rejected because a new group does not receive Ctrl+C at all.

### D2: Unix ignores SIGINT and SIGQUIT in the parent after the child has started

`std::process::Command::spawn` returns only after the child has `exec`ed, because std reports exec failure over a close-on-exec pipe. The child therefore starts with default dispositions. Only after that does the parent set `SIGINT` and `SIGQUIT` to `SIG_IGN` with `libc::sigaction`, saving the previous actions. It then calls `child.wait()`. The saved actions are restored when the same `ChildRunGuard` drops after the TUI resumes.

`libc` becomes a direct `cfg(unix)` dependency. It is already in the lock graph through crossterm, `mio`, and `signal-hook`, so no new crate is built.

*Alternative considered:* ignore the signals before spawning and reset them in the child with `pre_exec`. This was rejected because it is unsafe code in the fork path for no gain over ordering the calls.

### D3: Quit-time job shutdown in the TUI, whatever the core phase

- `JobHandle` gains `thread: JoinHandle<()>`.
- Every `if quit { return Ok(()) }` in `run`, plus the initial `run_effects`, goes through `shutdown(rt)`. If `rt.active_job` holds a thread that is not yet finished, `shutdown`:
  1. sets the cancel flag;
  2. polls `thread.is_finished()` every 10 ms for up to `QUIT_JOB_WAIT = 2 s`;
  3. joins the thread if it finished, or abandons it if not.
- No dialog answer is sent. The wait loop in D5 turns the cancel flag into an abort for any pending prompt, so a conflict and an error reply can never be mismatched. Today a stray `JobReply::Error` sent to a waiting conflict would become `Skip`.
- Because this works on `rt.active_job` rather than on core's `CancelJob` effect, it also covers a quit confirmed while core no longer shows `FileOpRunning`.
- The screen is not redrawn during the wait. At most 2 s pass with the last frame still showing.

*Alternative considered:* `join()` with no bound. This was rejected because a write blocked on an unresponsive SMB server could hang quit indefinitely. Exiting after 2 s leaves the same partial-file risk as today, and only in that degenerate case.

### D4: The panic hook acts only on the UI thread

- `install_panic_hook_with` captures `std::thread::current().id()` into the hook closure it installs. `run` already calls it on the UI thread (`app.rs:245`), before entering raw mode. Capturing in the closure, rather than in a process-wide `OnceLock`, keeps each installation self-contained, so the existing hook tests, which install from different harness threads, keep working.
- The hook runs `restore()` and then the previous hook only when the panicking thread is that thread.
- On any other thread it does neither. The default hook would print `thread '<unnamed>' panicked at …` onto the alternate screen while ratatui keeps diff-drawing over it.
- The payload message is recovered by the worker's own `catch_unwind` (D6). When `error-diagnostics`' log is enabled, it is written there.
- **Spec wording.** The MODIFIED "Panic hook restores the terminal before reporting" keeps both existing scenarios. The WHEN of "Panic in raw mode restores the terminal first" changes from "a panic occurs" to "a panic occurs on the UI thread". Unqualified, that scenario would require a worker panic to restore the terminal too, which is exactly the R-12 behavior this decision removes. The new scenario "A worker panic leaves the terminal alone" states the other half. "Original hook still runs" is unchanged: it is conditioned on the hook having restored the terminal, which now happens only on the UI thread.

*Alternative considered:* compare `thread::current().name() == Some("main")`, as the audit suggests. This was rejected because a `ThreadId` captured at install time is exact, and it does not depend on `run` being called from the thread std names `main`. Tests call `run`'s helpers from harness threads.

### D5: Prompts wait with a timeout loop that observes the cancel flag

`ChannelSink::conflict` and `ChannelSink::error` loop on `reply_rx.recv_timeout(100 ms)`:

- an answer that arrives returns as today;
- on `Timeout`, if `self.cancel.is_cancelled()`, return `ConflictChoice::Cancel` or `ErrorChoice::Abort`;
- on `Disconnected` (the main loop dropped the `JobHandle`), return `Cancel` or `Abort`. Today this falls back to `Skip`, so a job whose UI is gone stops instead of continuing unattended.

The 100 ms bound fits easily inside the 2 s quit budget.

*Alternative considered:* have `CancelJob` also push a synthetic reply. This was rejected because it needs to know which prompt is pending to avoid the mismatch described in D3.

### D6: Worker bodies run under `catch_unwind`, and each worker has a defined panic reply

A helper `fn guarded<T>(body: impl FnOnce() -> T) -> Result<T, String>` wraps `std::panic::catch_unwind(AssertUnwindSafe(body))` and turns the payload into a message (`&str`, `String`, or `"unknown panic"`). Each `spawn_*` uses it and maps a panic to its normal failure reply:

| Worker | Reply on panic |
|---|---|
| `spawn_listing` | `ListingFailed { panel, message: "Internal error while reading <path>: <msg>" }`, plus the request id once `listing-request-guard` lands |
| `spawn_job` | `JobDone { outcome: Cancelled { skipped: [SkippedItem { path: source_dir, reason: "internal error: <msg>" }] }, source_dir, dest_dir }`. The skipped list built up inside `run_job` is lost, but the summary dialog appears and the panels re-read. The sink's `tx` is cloned before `run_job` borrows the sink. |
| `spawn_git_info_query` | `GitInfoResolved { info: GitInfo::none() }` |
| `spawn_info_query` | `InfoResolved` with every value resolved to empty or zero, as for an unreadable directory |
| `spawn_drive_label` | `DriveLabelResolved { label: None }` |
| `spawn_tree_expand` | `TreeNodeExpanded { children: vec![] }` |
| `spawn_find_subtree` | `FindFileSearchDone { request }` |
| `spawn_viewer_search` | `ViewerSearchResult { offset: None, match_range: None, request }` |

`race_with_timeout`'s inner thread needs no wrapper (see Context), but the hook change in D4 is what keeps it from touching the terminal.

*Alternative considered:* a new `JobOutcome::Failed { message }`. This was rejected for now because it would touch core state and every outcome match. Reusing `Cancelled` with an explanatory skipped item gives the observable result the requirement asks for: the dialog closes, the panels re-read, and the internal error is named.

### D7: `ConflictChoice::Cancel` and the dialog's Cancel control

- `fs_ops::ConflictChoice` gains `Cancel`.
- `ConflictPolicy::apply` ignores it, so it never latches.
- Each conflict match in the engine (`run_copy_or_move`, `run_move_same_volume`, `run_rename`) maps `Cancel` to "set cancelled, stop", without pushing a `SkippedItem`, and the job ends with `JobOutcome::Cancelled { skipped }`.
- Input: with no rename field open, Esc and a plain `c`/`C` map to `FileOpConflictChoice(ConflictChoice::Cancel)`. This mirrors the progress dialog's own `Esc`/`c` cancel. The existing O/W/S/A/R letters are unaffected.
- Core's existing `FileOpConflictChoice` arm forwards the answer with `Effect::SendConflictReply` and returns the dialog to Progress until `JobDone` arrives. No core logic changes beyond a new `ButtonId::ConflictCancel` in `button_command`.
- The dialog renders a `(C)ancel` control on its own row below the existing choice row, so the existing row and its hit spans stay as they are, and the dialog grows by one row.

## Risks / Trade-offs

- [A Ctrl+C typed after `guard.suspend()` but before the child is spawned still takes the default path.] → The flag is set before `suspend` is called, so this window does not exist on Windows. On Unix the window from `suspend` to `spawn` returning is a few system calls long. It matches today's exposure, and nothing is lost: no child has started yet, so only FileCommand would exit, exactly as today.
- [A worker panic now prints nothing to the console.] → The message appears in the failure reply (the panel error or the summary item) and in the diagnostic log when it is enabled. That is strictly more visible than a report drawn under the TUI and then partly painted over.
- [The quit wait shows a frozen frame for up to 2 s.] → This only happens when quitting mid-job. A "Cancelling…" frame was considered unnecessary for a bounded 2 s wait.
- [The conflict dialog grows by one row, which changes snapshots.] → Accepted. The snapshots are updated deliberately.

## Migration Plan

None.

## Open Questions

None.
