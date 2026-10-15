# Change: error-diagnostics

## Why

Audit finding **B-06 (Low)**: errors are strings that lose their context, several failures are dropped silently, and there is no logging at all.

- **Silently dropped errors.** These failures disappear without trace:
  - persistence writes (`let _ = config::save_history_file_atomic(..)`, `save_theme_atomic`, and `save_panel_split_atomic` at `app.rs:897`, `:906`, `:913`);
  - channel sends (`let _ = rt.tx.send(..)` and `handle.reply_tx.send(..)` throughout `app.rs` and `worker.rs`, for example `app.rs:822`, `:857`, `:862`);
  - metadata restoration after each copied file (`fs_ops/worker.rs:289-298`): the alternate-data-stream listing falls back to an empty list on error, each stream copy, the modification-time restore, and the read-only restore all use `let _ =`;
  - startup warnings, because `State::startup_warning` holds only one: each later warning overwrites the earlier ones (`app.rs:288-322`, plus `apply_user_menu` at `:696`).
- **Status-line errors without context.** The panel and status-line errors that come from `io::Error::to_string()` do not say what failed:
  - `ListingFailed { message: e.to_string() }` (`worker.rs:199`);
  - `ViewerOpenFailed` (`app.rs:937`);
  - `EditorOpenFailed` (`app.rs:979`);
  - `EditorSaveFailed` (`app.rs:992`).

  A user report of "Access is denied. (os error 5)" therefore does not say what was denied.
- **No diagnostic trail.** There is no `log`/`tracing` crate or debug file anywhere, so a user report cannot be diagnosed after the fact.

The impact is partial failures nobody notices (for example ADS not copied, or timestamps not restored) and bug reports nobody can act on.

## What Changes

- **Optional diagnostic log.** A diagnostic log is enabled with `--log <path>` (or `--log=<path>`) or the `FILECOMMAND_LOG` environment variable; the flag wins. When it is enabled, each swallowed error is appended to the file with a timestamp and a category:
  - persistence write failures;
  - failed channel sends;
  - metadata-restore failures;
  - every startup warning.

  When neither is set, no file is created or written. The logger is a minimal mutex-guarded file appender in `filecommand-tui`. No new crates are added and `filecommand-core` gains no logging dependency. A log path that cannot be opened raises the startup-warning dialog, and the session continues without logging. `--help` lists the flag and the variable.
- **Status-line errors name what failed.**
  - A panel read error reads `Cannot read <directory>: <OS message>`.
  - Viewer and editor open failures read `Cannot open <file>: <OS message>`.
  - A built-in editor save failure reads `Cannot save <file>: <reason>`, where the reason is the OS message or the editor's own refusal text.

  Messages that already name their target (`cd`/`del`/`rmdir` rejections, Open-with failures) are unchanged. The error-recovery dialog (path above the message) and the end-of-job summary rows (`<path>: <reason>`) already name the full path, and that behavior is now required.
- **Metadata-restore warnings reach the user.** A copied file whose alternate data streams, modification time, or read-only attribute could not be restored is recorded as a warning in the end-of-job summary, separate from skipped items. The file still counts as copied. A job that skipped nothing but has warnings still shows the summary, titled by its warning count.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `application-shell`: ADDED "Optional diagnostic log". This covers the flag and environment variable, what is logged and its format, behavior when the log is unset or unwritable, and the `--help` listing. The listing follows the precedent of the existing `--theme` entry and the in-flight `--noanimations` change, so "Help flags print usage and exit before TUI startup" is left unchanged. (If the `config-profile-directory` change has already landed, `--config-dir` is an equally good model for the listing.)
- `filesystem-error-handling`: ADDED "Error messages name the affected path".
- `operation-dialogs`: ADDED "Metadata-restore failures are reported in the end-of-job summary". MODIFIED "End-of-job skipped-files summary": the summary is also shown for a job that recorded warnings but skipped nothing, so its "No summary when nothing was skipped" scenario now reads "no items skipped and no warnings recorded". Its other scenario is kept verbatim, and a warnings-only scenario is added.

## Impact

- New `crates/filecommand-tui/src/diag.rs`: `init(path) -> io::Result<()>`, `log(category, message)`, and a `log_err` helper. It holds a `OnceLock<Mutex<File>>`, opened in append mode, and formats a UTC timestamp through `listing::DateTime::from_system_time`.
- `crates/filecommand-tui/src/app.rs`:
  - `LaunchOptions`/`parse_launch_args` (`:78-145`) gain `log: Option<String>`;
  - `run` resolves `--log` over `FILECOMMAND_LOG` and calls `diag::init`, logging every startup warning before `stamp_startup_warning`;
  - `run_effects` logs the `Persist*` failures (`:891-915`) and every `send` failure;
  - the `ViewerOpenFailed`, `EditorOpenFailed`, and `EditorSaveFailed` message formats change.
- `crates/filecommand-tui/src/main.rs`: `print_usage` lists `--log <path>` and `FILECOMMAND_LOG`.
- `crates/filecommand-tui/src/worker.rs`: the `ListingFailed` message names the directory. Failed `tx.send`s are logged. `ChannelSink::done` logs the job's metadata warnings.
- `crates/filecommand-core/src/fs_ops/error.rs`: `SkippedItem` gains a `kind: ItemKind { Skipped, Warning }` with `SkippedItem::skipped(..)` and `SkippedItem::warning(..)` constructors.
- `crates/filecommand-core/src/fs_ops/worker.rs`: `copy_one_file` (`:288-299`) records a warning for each failed ADS listing, stream copy, mtime restore, or read-only restore instead of discarding it. The sink-facing outcome carries them in the existing `skipped` vector.
- `crates/filecommand-tui/src/views/skipped_summary.rs`: groups skipped items and warnings and titles the dialog by both counts.
- `README.md`: a "Diagnostics" note.
- Tests:
  - argument parsing and environment precedence;
  - the logger appends with a timestamp, and creates nothing when it is disabled;
  - startup warning when the log cannot be opened;
  - message formats;
  - FakeFs-injected `set_modified`/`copy_stream_chunked`/`list_streams` failures produce warnings and still count the file as copied;
  - summary snapshots for warnings only and for skipped items plus warnings.

**Audit findings:** B-06

**Coordination:**
- `durable-file-copy` (R-07/R-08/R-14/B-01) moves Windows copies to `CopyFileExW`, which carries ADS, the read-only/hidden/system/archive attributes, and the last-write time, and restores the creation time explicitly. ACLs and owner are an explicit non-goal there (the copy inherits the destination's permissions), so no ACL warning exists in either change. It removes the worker's restore loop (and `list_streams`/`copy_stream_chunked`/`set_modified` from the `FileSystem` trait) and returns whatever could not be carried over in `CopyReport::unrestored`. Whichever change lands second maps `unrestored` entries onto this change's warning items (and the per-volume "streams not supported" aggregation), and updates the "Metadata-restore failures" scenarios and the FakeFs fault-injection tests to the set of metadata that change restores.
- `copy-onto-itself-guard` adds another engine-recorded skip ("source and destination are the same file") listed in the end-of-job summary. It relies on this change's "for example via …" wording in the MODIFIED "End-of-job skipped-files summary"; such items are `Skipped`, not warnings. No ordering constraint.
- `process-lifecycle-hardening` (R-10/R-11/R-12/R-18): its contained worker panics and its quit-wait timeout are logged through `diag::log` when the log is enabled. Whichever change lands second wires them in.
- `config-parsing-robustness` (R-15/R-16/B-05) collects parser warnings and surfaces them at startup. They pass through the same startup-warning path, so this change logs every one, including any a later warning overwrites.
- `history-persistence-hardening` (P-08/R-13/R-15) may move the persistence writes off the UI thread. Their failures must still go through `diag::log`.
- `offload-blocking-io` (R-09/P-09) moves the viewer and editor open and save onto workers. The `Cannot open`/`Cannot save` formats apply to whichever code builds those messages.
- `editor-encoding-safety` (R-02) makes a read-only save fail with its own message (`<name> is read-only — not saved`) instead of an OS error. Under this change that message is the `<reason>` in `Cannot save <file>: <reason>`. Whichever change lands second keeps the read-only text intact inside the prefix.
