# Design: error-diagnostics

## Context

Verified on `build/open-proposals-2026-10` (`1bada6f`).

- **Persistence failures are discarded.** `run_effects` runs `let _ = config::save_history_file_atomic(..)` (`app.rs:897`), `let _ = config::save_theme_atomic(..)` (`:906`), and `let _ = config::save_panel_split_atomic(..)` (`:913`). The comments say this is deliberate, since a failed write must not end the session, but nothing records the failure.
- **Channel sends are discarded.** Every `Command` reply in `run_effects` and in each `worker::spawn_*` is sent with `let _ = tx.send(..)`; examples are `app.rs:822`, `:876`, `:920`, `:934`, and `worker.rs:41-199`. The job answers use `let _ = handle.reply_tx.send(..)` (`app.rs:857`, `:862`).
  - A send to `rt.tx` from the UI thread can only fail after the receiver in `run` is gone.
  - A worker's send fails after shutdown.
  - A `reply_tx` send fails when the job thread has already ended.

  These are rare, but each is a lost result that cannot be diagnosed today.
- **Metadata-restore failures are discarded** (`fs_ops/worker.rs:288-298`, inside `copy_one_file`, after a successful `copy_file_chunked`):
  - `fs.list_streams(src).unwrap_or_default()` turns a listing error into "no streams";
  - `let _ = fs.copy_stream_chunked(..)` discards each stream failure;
  - `let _ = fs.set_modified(dst, ..)` and `let _ = fs.set_readonly(dst, true)` discard restore failures.

  The file is then counted as done (`CopyOneResult::Done`).
- **Status-line messages carry no context.**
  - `spawn_listing` sends `ListingFailed { message: e.to_string() }` (`worker.rs:199`), which core stores verbatim in `last_error` (`update.rs:3905-3909`). The panel title does show the failed directory, because `begin_new_listing` sets `cwd` before the read (`panel.rs:498-516`), but the message itself does not.
  - `ViewerOpenFailed`, `EditorOpenFailed`, and `EditorSaveFailed` likewise carry the bare `e.to_string()` (`app.rs:937`, `:979`, `:992`).
- **The job dialogs already show paths, contrary to the audit's wording.**
  - The error-recovery dialog draws `info.path` on its first row and `info.message` on the second (`views/error_dialog.rs:18-41`).
  - The end-of-job summary formats each row as `"{path}: {reason}"` (`views/skipped_summary.rs:22`).

  For job errors, B-06's "job error messages lack the path" is therefore already met. This change pins that behavior in the spec.
- **Startup warnings are last-writer-wins.** `State::startup_warning` is a single `Option<String>`. `run` assigns it up to five times (`app.rs:288-322`), and `apply_user_menu` once more (`:696`), so earlier warnings are lost.
- **No logging infrastructure.** There is no `log`/`tracing` dependency and no debug file. Launch flags are parsed by `parse_launch_args` (`app.rs:117-145`), and `main.rs` prints the usage block by hand.

## Goals / Non-Goals

**Goals:**

- A zero-cost, opt-in, append-only diagnostic file that records every error the application swallows.
- Status-line errors say what failed.
- Metadata that is not restored becomes visible to the user as a warning.
- No new crates, and no logging in `filecommand-core`.

**Non-Goals:**

- Typed error enums replacing `String` messages. B-06 calls error handling "stringly-typed", but the user-visible requirement is context, not types. A typed-error refactor would touch every reply command and is not needed to satisfy it.
- Log levels, log rotation, or size caps. The log is opt-in for diagnosing a specific problem.
- Logging successful operations or a trace of every command.
- The `CopyFileExW` metadata path and the creation-time restore. Those are `durable-file-copy`'s, which hands its failures over in `CopyReport::unrestored`. ACLs and owner are not copied by either change (a `durable-file-copy` non-goal), so they produce no warning.
- Worker panics and quit-wait timeouts. `process-lifecycle-hardening` owns them and logs through this change's API.

## Decisions

### D1: A minimal file appender in `filecommand-tui`

New module `diag.rs`:

- `static LOG: OnceLock<Mutex<std::fs::File>>`.
- `pub fn init(path: &Path) -> io::Result<()>` opens the file with `OpenOptions::new().create(true).append(true)` and writes a session header line: `FileCommand <version> session start`.
- `pub fn log(category: &str, message: impl Display)` is a no-op when `LOG` is unset. Otherwise it locks, writes one `"{ts} [{category}] {message}\n"` line, flushes, and ignores write errors.
- `pub fn log_err<E: Display>(category, context, result: Result<T, E>) -> Option<T>` is a convenience for the `let _ =` sites.

The timestamp is UTC, formatted `YYYY-MM-DDTHH:MM:SS.mmmZ` from `listing::DateTime::from_system_time(SystemTime::now())` plus the millisecond remainder. That reuses the civil-date conversion the codebase already has. The `Mutex` makes the log safe to call from worker threads. Append mode keeps concurrent sessions that share one file from clobbering each other.

*Alternative considered:* `log` plus `simplelog`, or `tracing`. This was rejected because the audit and the user asked for no new crates, and a one-file appender covers the need.

*Alternative considered:* logging from `filecommand-core`. This was rejected because core is I/O-free (application-shell "Pure core update function"). Core errors surface as reply commands or `SkippedItem`s, and the TUI logs them.

### D2: Activation, precedence, and failure handling

- `parse_launch_args` accepts `--log <path>` and `--log=<path>` into `LaunchOptions::log: Option<String>`, using the same absent/empty/value convention as `--theme` (`None` when the switch is absent, `Some(String::new())` when passed with no value, `Some(path)` otherwise). If the `config-profile-directory` change has already landed, `--config-dir` is an equally good model.
- `run` resolves the effective path: the flag if it is present, else the non-empty `FILECOMMAND_LOG`. Reading the environment happens in `run`, never in core.
- If the effective value is empty, or `diag::init` fails, `run` sets the startup warning `Diagnostic log disabled: cannot open <path> (<error>)`, or `--log needs a path`, and continues.
- `print_usage` gains the line `--log <path>        Append diagnostic messages to <path> (or set FILECOMMAND_LOG)`, in the same OPTIONS block that lists `--theme` (and that the in-flight `--noanimations` change extends; if `config-profile-directory` has landed, `--config-dir` is an equally good model).

### D3: What is logged

| Site | Category | Message |
|---|---|---|
| `Persist*` failures (`app.rs:891-915`) | `persist` | `history write <path>: <err>` / `config write <path>: <err>` |
| Failed `rt.tx.send` / worker `tx.send` | `channel` | `<command kind> not delivered: event loop gone` |
| Failed `reply_tx.send` | `job` | `conflict/error answer not delivered: job ended` |
| Metadata warnings in a `JobDone` outcome (`ChannelSink::done`) | `copy` | `<path>: <what> not restored: <err>` |
| Every startup warning, at the point it is assigned | `startup` | the warning text |

Startup warnings are logged at assignment, through a small `set_startup_warning(&mut state, msg)` helper, so that the earlier warnings the dialog drops are kept. `config-parsing-robustness`' collected parser warnings pass through the same helper.

### D4: Status-line message formats

- `spawn_listing` sends `Cannot read <name>: <err>`. `<name>` is `path.file_name()`, or the whole path when there is none (`C:\`, `\\server\share`).
- `ViewerOpenFailed` and `EditorOpenFailed` send `Cannot open <name>: <err>`, and `EditorSaveFailed` sends `Cannot save <name>: <err>`. After `editor-encoding-safety`, `<err>` for a read-only target is that change's own `<name> is read-only — not saved` text rather than an OS message; the prefix is added around it unchanged.
- The other status-line errors already name their target and are left alone: the command line's `cd`/`del`/`rmdir` rejections format `path.display()`, and `open_with_association_error` names the file. These four `e.to_string()` sites are the only bare OS messages that reach a status line (a search of `update.rs`, `app.rs`, and `worker.rs` for `last_error`/`save_error` assignments).
- The name is used rather than the full path because the panel's mini-status is about half the terminal width and truncates from the right. A full path would push the operating-system message, the actionable part, out of view, and the panel title directly above already shows the directory. The dialogs, which have room, keep showing full paths. Full paths also go to the log whenever a logged event carries one.

*Alternative considered:* `<OS message> (<full path>)`. This was rejected because it reads poorly and still truncates the path away at 80 columns.

### D5: Metadata warnings ride in the existing skipped list

- `SkippedItem` gains `kind: ItemKind { Skipped, Warning }`, with `SkippedItem::skipped(path, reason)` and `SkippedItem::warning(path, reason)` constructors. Every existing literal becomes `skipped(..)`.
- `copy_one_file` gains `warnings: &mut Vec<SkippedItem>`. It pushes:
  - `warning(dst, "alternate data streams not listed: <err>")` when `list_streams` fails;
  - `warning(dst, "stream <name> not copied: <err>")` for each failed stream;
  - `warning(dst, "modification time not restored: <err>")`;
  - `warning(dst, "read-only attribute not restored: <err>")`.

  It still returns `Done`.
- When the destination volume reports no named-stream support, the per-file stream copies are skipped and a single job-level warning is recorded instead (see Risks). `FileSystem` gains `supports_named_streams(&Path) -> io::Result<bool>`, and an error there is treated as "supported", so the per-file path still reports real failures.
- The engine appends these to the outcome's `skipped` vector, so `JobOutcome`, `Command::JobDone`, and `UiPhase::FileOpSummary(Vec<SkippedItem>)` keep their shapes. `handle_file_op_running` already shows the summary whenever that vector is non-empty, which gives the "warnings only" case for free.
- `skipped_summary.rs` lists `Skipped` rows first, then a `Warnings` group, and titles the dialog `Skipped N item(s)`, `N warning(s)`, or `Skipped N item(s), M warning(s)`.
- This change does not alter the cross-volume move's policy for removing the source after a file with warnings. That policy is today's (remove the source) until `durable-file-copy` decides otherwise.

*Alternative considered:* a separate `warnings: Vec<JobWarning>` in `JobOutcome` and `FileOpSummary`. This was rejected for churn: every outcome match and the summary phase would change. The `kind` field gives the same separation with one new field.

## Risks / Trade-offs

- [The existing operation-dialogs scenario "No summary when nothing was skipped" would contradict a warnings-only summary.] → This change MODIFIES "End-of-job skipped-files summary": its description gains the warnings case, and that scenario's WHEN becomes "no items skipped and no warnings recorded". Its intent holds, because a job with neither still shows nothing. The parenthetical list of skip sources becomes "for example via …", because `durable-file-copy`, `move-reparse-point-safety`, `process-lifecycle-hardening`, and `copy-onto-itself-guard` also list engine-skipped items (a failed verification, a link in a move, an internal error, a copy onto its own source) in this summary. No other new or in-flight change modifies this requirement (checked with `grep -rn "### Requirement: End-of-job skipped-files summary" openspec/changes/*/specs/`).
- [Logging from many threads through one `Mutex`.] → Only failures are logged, so contention is negligible. When the log is disabled, the cost is a `OnceLock` load.
- [ADS-heavy copies, such as browser downloads that each carry a `Zone.Identifier`, would produce one warning per file on a destination without stream support, such as a FAT32 or exFAT USB stick.] → The engine asks the `FileSystem` seam once per job whether the destination volume supports named streams (on Windows, `GetVolumeInformationW`'s `FILE_NAMED_STREAMS` flag). When it does not, the engine skips the per-file stream copies and records one job-level warning instead: the destination path plus "alternate data streams not supported; streams of N file(s) not copied". FakeFs gets a matching toggle.
- [Startup warnings logged before `diag::init`.] → `run` resolves and initializes the log first, before profile resolution and config loading, so every warning assignment comes after `init`.

## Migration Plan

None. The log is opt-in. The message text changes are cosmetic, and any snapshot or test asserting the old bare messages is updated.

## Open Questions

None.
