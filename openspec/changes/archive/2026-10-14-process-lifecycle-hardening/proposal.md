# Change: process-lifecycle-hardening

## Why

Four audit findings share one theme: FileCommand does not control how it, its child processes, and its worker threads stop.

- **R-10 (High): no console Ctrl+C handler.** While a command from the Run entry or the F2 user menu, or the external editor, runs in the suspended terminal (`app.rs:1041-1056`, `:1147-1157`), raw mode is off. Ctrl+C therefore becomes a console control event that Windows delivers to every process attached to the console, and Unix delivers SIGINT to the foreground process group. FileCommand installs no handler anywhere in the workspace, so pressing Ctrl+C to stop `ping -t` also kills FileCommand, along with its tabs and session state.
- **R-11 (Medium): quit does not wait for job cleanup.** `ConfirmQuit` emits `[CancelJob, Quit]` (`update.rs:1299-1309`). `run_effects` sets the cancel flag (`app.rs:850-854`) and `run` returns at once (`app.rs:391-394`). `JobHandle` keeps no `JoinHandle` (`worker.rs:216-219`), so the process can exit while the job thread is in the middle of `write_all`. The cancel path that removes the partial file (`fs_ops/fs.rs:263-268`) never runs, and a truncated destination file remains. This drifts from application-shell "Quit request keys and confirmation", which says the job is aborted through its cancel semantics *before* exit.
- **R-12 (Medium): the panic hook restores the terminal whatever thread panics.** `std::panic::set_hook` is process-wide (`terminal.rs:219-225`). A panic in a listing, git, find, or job worker leaves raw mode and the alternate screen while the UI keeps drawing, and the chained default hook prints the report across the TUI. Separately, a panicked listing worker never sends `ListingComplete`, so the panel shows `Reading… N` forever. A panicked job never sends `JobDone`, so the progress dialog never closes.
- **R-18 (Low): no cancel from the conflict dialog, and cancellation is not seen while a job waits.** The overwrite-conflict dialog maps only O/W/S/A/R (`input/mod.rs:824-831`), so Esc does nothing. The job thread is parked in `reply_rx.recv()` (`worker.rs:234-254`), so a cancel request is not seen until some answer arrives. To abort a long copy from a conflict prompt, the user must choose Skip All and then Cancel.

## What Changes

- **Ctrl+C reaches only the child.** While a child started by the Run entry, the F2 user menu, or the F4 external editor is running, FileCommand ignores Ctrl+C and Ctrl+Break: a console control handler on Windows, SIGINT and SIGQUIT ignored in the parent on Unix, set up only after the child has started. The child still receives the signal and stops normally, then FileCommand shows its press-any-key prompt and resumes.
- **Quit waits for job cleanup.** Confirming quit during a job sets the cancel flag. Any conflict or error prompt the job is waiting on resolves as an abort without needing an answer. Quit then waits up to about 2 s for the job thread to finish its cancellation cleanup, including removing the partial file, before exiting. This applies to any live job thread, whatever screen core thinks is active.
- **Worker panics stay in their worker.** The panic hook restores the terminal and chains to the previous hook only on the UI thread. A worker panic leaves the terminal alone and prints nothing over the TUI. Each worker body runs under `catch_unwind`, and a panic becomes that worker's normal failure result:
  - a listing reports a failed listing;
  - a job ends as cancelled, with the internal error in the end-of-job summary;
  - query workers return their "no result" answer.
- **The conflict dialog gets Cancel.** It adds Cancel, chosen with Esc, `C`, or a click. Cancel aborts the job through the same cancelled outcome as the progress dialog's Cancel. A job waiting on a conflict or error answer now checks the cancel flag while it waits. Esc on the error-recovery dialog, which already chooses Abort in code, is pinned by a scenario.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `application-shell`:
  - ADDED "Ctrl+C during a suspended child run stops only the child".
  - ADDED "Quit waits for job cancellation cleanup". "Quit request keys and confirmation" already requires the abort before exit and is left unchanged.
  - MODIFIED "Panic hook restores the terminal before reporting": restore and report on the UI thread only, worker panics contained and surfaced. Its scenarios are kept; the first is deliberately qualified to say "on the UI thread", and new scenarios are added.
- `operation-dialogs`:
  - MODIFIED "Overwrite-conflict dialog": a Cancel choice via Esc, `C`, or the Cancel control. All existing scenarios are kept verbatim.
  - ADDED "Cancellation is observed while a job waits for an answer", which covers both the conflict and error-recovery dialogs and pins Esc as Abort on the error-recovery dialog. "Error-recovery dialog" is left unchanged.

## Impact

- `crates/filecommand-tui/src/app.rs`:
  - `run_shell_command` (`:1041-1056`) and `spawn_editor_and_wait` (`:1165-1176`) switch from `.status()` to `spawn()` plus `wait()` inside a child-run guard.
  - `run` installs the console control handler and performs the bounded job shutdown before returning on quit (`:391-394` and every other `quit` return).
  - The `Effect::RunJob`/`CancelJob` handling (`:849-854`) is affected.
- `crates/filecommand-tui/src/terminal.rs`: `install_panic_hook_with` (`:219-225`) records the UI `ThreadId` and only restores and chains on that thread.
- `crates/filecommand-tui/src/worker.rs`:
  - `JobHandle` gains `thread: JoinHandle<()>` (`:216-219`, `spawn_job` `:408-421`).
  - `ChannelSink::conflict` and `ChannelSink::error` wait with `recv_timeout` in a loop that checks the cancel flag (`:234-254`).
  - Every `spawn_*` body is wrapped in `catch_unwind`, with a per-worker failure reply.
  - A new `console` helper module (or a section of `app.rs`) holds the Windows ctrl handler and the Unix signal guard.
- `crates/filecommand-core/src/fs_ops/conflict.rs`: `ConflictChoice::Cancel`.
- `crates/filecommand-core/src/fs_ops/worker.rs`: every match on a conflict answer (`run_copy_or_move` `:386-403`, `run_move_same_volume` `:472-487`, `run_rename` `:550-561`) handles `Cancel` as "stop, job cancelled".
- `crates/filecommand-core/src/update.rs`: `ButtonId::ConflictCancel` and the `button_command` mapping.
- `crates/filecommand-tui/src/input/mod.rs`: `map_file_op_running_key` (`:800-835`) maps Esc and `C` to `FileOpConflictChoice(Cancel)` when no rename field is open.
- `crates/filecommand-tui/src/views/conflict_dialog.rs`: renders and hit-tests the Cancel control.
- `Cargo.toml` (tui): the `windows-sys` features gain `Win32_System_Console`. A direct `[target.'cfg(unix)'.dependencies] libc` is added; it is already in the lock graph through crossterm, `mio`, and `signal-hook`.
- Tests:
  - engine tests with FakeFs for `Cancel`;
  - `ChannelSink` tests for cancel-while-waiting and disconnected-reply;
  - panic-hook tests for UI versus worker threads;
  - `catch_unwind` reply tests per worker;
  - input mapping and conflict-dialog snapshots;
  - Windows integration tests for the ctrl handler gate and for the quit-time wait removing a partial file.

**Audit findings:** R-10, R-11, R-12, R-18

**Coordination:**
- `listing-request-guard` (R-04) adds a request id to `ListingChunk`, `ListingComplete`, and `ListingFailed`. The panic path's `ListingFailed` must carry it. Whichever change lands second adds the id to the `catch_unwind` reply.
- `durable-file-copy` (R-07/R-08/R-14/B-01) changes the copy to write a temporary file and rename it. Its cancellation cleanup, which removes the temporary or partial file, is what the quit-time wait gives time to run. The "partial file removed" scenario holds for either implementation.
- `resize-preserves-session-state` (R-03): this change's quit-time shutdown acts on `rt.active_job` whatever the core phase, so it also covers the quit-from-placeholder path. Either order works.
- `error-diagnostics` (B-06): when its diagnostic log is enabled, worker panic messages and the quit-wait timeout are logged there. Whichever change lands second wires them in.
- `offload-blocking-io` (R-09/P-09) adds new workers (path probe, viewer open, editor open and save, shell launch). They must use the same `catch_unwind` wrapper. Whichever change lands second applies it.
