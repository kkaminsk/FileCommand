# Tasks: process-lifecycle-hardening

## 1. Ctrl+C during suspended child runs (`crates/filecommand-tui`)

- [x] 1.1 `Cargo.toml`: add `Win32_System_Console` to the `windows-sys` features. Add `[target.'cfg(unix)'.dependencies] libc` and confirm with `cargo tree` that no new crate is built (design D1, D2).
- [x] 1.2 Add a `console` module with:
  - `static CHILD_RUNNING: AtomicBool`;
  - on Windows, `install_ctrl_handler()` and an `extern "system" fn ctrl_handler(u32) -> BOOL` that returns `TRUE` only for `CTRL_C_EVENT` and `CTRL_BREAK_EVENT` while the flag is set;
  - an RAII `ChildRunGuard` that sets and clears the flag, and on Unix also holds and restores the saved `SIGINT`/`SIGQUIT` `sigaction`s.

  (application-shell "Ctrl+C during a suspended child run stops only the child")
- [x] 1.3 `app::run`: call `console::install_ctrl_handler()` once at startup, on Windows only.
- [x] 1.4 `run_shell_command` and `run_external_editor`/`spawn_editor_and_wait`:
  - create the guard before `guard.suspend()` and hold it until after `guard.resume()`;
  - replace `.status()` with `spawn()` followed by `wait()`;
  - on Unix, have the guard ignore `SIGINT`/`SIGQUIT` only after `spawn` returns.

  The existing spawn-failure messages are unchanged (design D1, D2).
- [x] 1.5 Unit tests: `ctrl_handler` returns `TRUE` for Ctrl+C and Ctrl+Break only while the guard is alive, and `FALSE` for `CTRL_CLOSE_EVENT` at all times. The guard clears the flag on drop, including during an unwind.
- [x] 1.6 Unix integration test: inside a `ChildRunGuard` after spawning `sleep 5`, `kill -INT` the process group. The child exits and the test process survives. Mark it `#[cfg(unix)]`.

## 2. Quit waits for job cleanup

- [x] 2.1 `worker.rs`: add `thread: JoinHandle<()>` to `JobHandle` and have `spawn_job` store it (design D3).
- [x] 2.2 `app.rs`: add `shutdown(rt)`. It sets the cancel flag on a live job, polls `is_finished()` every 10 ms for up to `QUIT_JOB_WAIT` (2 s), then joins or abandons the thread. Route every `quit` return in `run`, including the initial `run_effects`, through it (application-shell "Quit waits for job cancellation cleanup").
- [x] 2.3 Tests:
  - a fake job thread that finishes 300 ms after the flag is set makes `shutdown` return after it finishes;
  - one that never finishes makes `shutdown` return after about 2 s;
  - with no job, `shutdown` returns immediately.
- [x] 2.4 Windows integration test: start a real `spawn_job` copying a large temp file to a destination, call `shutdown` mid-copy, then assert that no destination file remains.

## 3. Cancellation while a job waits on a prompt

- [x] 3.1 `ChannelSink::conflict` and `ChannelSink::error` loop on `recv_timeout(100 ms)`. On a timeout with the cancel flag set, return `ConflictChoice::Cancel` or `ErrorChoice::Abort`. On `Disconnected`, return `Cancel` or `Abort` (design D5; operation-dialogs "Cancellation is observed while a job waits for an answer").
- [x] 3.2 Tests on `ChannelSink` with a real channel:
  - setting the flag while `conflict()` waits returns `Cancel` within about 200 ms;
  - setting it while `error()` waits returns `Abort`;
  - dropping the reply sender returns `Cancel` or `Abort`;
  - a normal answer is still returned unchanged.

## 4. Conflict-dialog Cancel

- [x] 4.1 `fs_ops/conflict.rs`: add `ConflictChoice::Cancel`. `ConflictPolicy::apply` ignores it (design D7).
- [x] 4.2 `fs_ops/worker.rs`: handle `Cancel` in `run_copy_or_move`, `run_move_same_volume`, and `run_rename`. Each stops, pushes no `SkippedItem` for the conflicting item, and ends with `JobOutcome::Cancelled`.
- [x] 4.3 Engine tests with FakeFs and `RecordingSink`:
  - a three-file copy whose second file conflicts, answered `Cancel`, leaves the target unchanged, does not copy the third file, gives the outcome `Cancelled { skipped: [] }`, and makes no write to the conflicting target;
  - the same for a same-volume move and for a rename.
- [x] 4.4 `input/mod.rs`: with no rename field open, Esc and a plain `c`/`C` map to `FileOpConflictChoice(ConflictChoice::Cancel)`. With the rename field open, Esc keeps mapping to `FileOpCancel`. Add tests for both, and pin the error dialog's existing Esc to Abort mapping with a test.
- [x] 4.5 `update.rs`: add `ButtonId::ConflictCancel` and map it to `FileOpConflictChoice(Cancel)` in `button_command`. Add a reducer test that a `Cancel` answer emits `SendConflictReply(Cancel)` and returns the dialog to Progress.
- [x] 4.6 `views/conflict_dialog.rs`: render a `(C)ancel` row below the choice row and record its hit span. Update the conflict-dialog snapshots and add one with the Cancel row.

## 5. Panic containment

- [x] 5.1 `terminal.rs`: `install_panic_hook_with` captures the installing thread's `ThreadId` in the hook closure (not a process-wide `OnceLock`, which the multi-threaded test harness could not reset). The hook runs `restore()` and the previous hook only on that thread (design D4; application-shell "Panic hook restores the terminal before reporting").
- [x] 5.2 Tests extending `panic_inside_guarded_scope_restores_terminal_before_report_prints`:
  - a panic on the installing thread records "restore" before "report";
  - a panic on a spawned thread, caught by joining, records neither;
  - restore the original hook afterwards.
- [x] 5.3 `worker.rs`: add the `guarded(body)` helper and wrap every `spawn_*` body. Send the per-worker failure reply from the design D6 table.
- [x] 5.4 Tests with an injected panicking body (a test-only seam that takes a closure):
  - listing gives `ListingFailed` naming the path;
  - job gives `JobDone { Cancelled { skipped: [internal error] } }`;
  - each query worker gives its "no result" reply.
- [x] 5.5 Reducer tests: `ListingFailed` after chunks clears `Streaming`. A `JobDone(Cancelled)` carrying an internal-error item leaves `FileOpRunning` for `FileOpSummary` and emits the re-read listings.

## 6. Verification

- [x] 6.1 `cargo test --workspace` passes.
- [x] 6.2 `cargo clippy --workspace --all-targets` reports no new warnings.
- [x] 6.3 `openspec validate process-lifecycle-hardening --strict` is clean.
- [ ] 6.4 Manual (Windows):
  - Add the user-menu entry `ping -t 127.0.0.1`, run it, and press Ctrl+C. Ping stops, the prompt appears, and FileCommand resumes with its tabs.
  - Copy a multi-GB file, press F10 and confirm. No partial file remains at the destination.
  - Trigger a conflict and press Esc. The job ends and the panels re-read.
