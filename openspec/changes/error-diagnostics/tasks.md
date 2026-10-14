# Tasks: error-diagnostics

## 1. Diagnostic logger (`crates/filecommand-tui/src/diag.rs`)

- [x] 1.1 Add `diag.rs`:
  - a `OnceLock<Mutex<File>>`;
  - `init(path)`, which opens in append mode and writes the session header;
  - `log(category, message)`, which writes a UTC `YYYY-MM-DDTHH:MM:SS.mmmZ` timestamp via `listing::DateTime::from_system_time`, flushes, and ignores write errors;
  - the `log_err` helper.

  No new dependencies (design D1; application-shell "Optional diagnostic log").
- [x] 1.2 Unit tests with an injected path:
  - a disabled logger creates no file;
  - after `init`, two `log` calls append two lines that each start with a timestamp matching the format;
  - re-initializing on an existing file keeps its earlier content;
  - logging from four threads at once produces whole lines.

## 2. Activation (`app.rs`, `main.rs`)

- [x] 2.1 `LaunchOptions::log` and `parse_launch_args`: accept `--log <path>` and `--log=<path>`, using `--theme`'s absent/empty/value convention (if the `config-profile-directory` change has already landed, `--config-dir` is an equally good model). Add parser tests (design D2).
- [x] 2.2 `run`: before profile resolution, resolve the flag over a non-empty `FILECOMMAND_LOG` and call `diag::init`. An empty value or an `init` error sets a startup warning and continues without logging. Add tests for the precedence and failure paths via a pure `resolve_log_path(flag, env)` helper.
- [x] 2.3 Route every startup-warning assignment in `run` and `apply_user_menu` through a `set_startup_warning` helper that also logs the warning (design D3).
- [x] 2.4 `main.rs` `print_usage`: add the `--log <path>` line mentioning `FILECOMMAND_LOG`. Extend the help-output test.

## 3. Swallowed-error sites

- [x] 3.1 `run_effects`: log `PersistHistory`, `PersistTheme`, and `PersistPanelSplit` failures with the target path and the error. Log every failed `rt.tx.send` and `handle.reply_tx.send` (design D3).
- [x] 3.2 `worker.rs`: log failed `tx.send` calls in every `spawn_*` body and in `ChannelSink`. In `ChannelSink::done`, log each `Warning` item of the outcome under `copy`.

## 4. Status-line messages name the item (filesystem-error-handling "Error messages name the affected path")

- [x] 4.1 `spawn_listing`: `ListingFailed` message `Cannot read <name>: <err>`, falling back to the whole path when `file_name()` is `None` (design D4).
- [x] 4.2 `run_effects`: `ViewerOpenFailed` and `EditorOpenFailed` become `Cannot open <name>: <err>`, and `EditorSaveFailed` becomes `Cannot save <name>: <err>`.
- [x] 4.3 Tests:
  - a listing of a missing directory yields `Cannot read <name>: …`;
  - a root path keeps the whole path as the name;
  - the editor and viewer failure formats;
  - snapshot tests pin the error dialog's path row and the summary's `path: reason` rows. They already pass and are kept as regressions.

## 5. Metadata-restore warnings (operation-dialogs "Metadata-restore failures are reported in the end-of-job summary", "End-of-job skipped-files summary")

- [x] 5.1 `fs_ops/error.rs`: add `ItemKind { Skipped, Warning }` and `SkippedItem::kind`, with `skipped(..)` and `warning(..)` constructors. Migrate the existing literals to `skipped(..)` (design D5).
- [x] 5.2 `fs_ops/fs.rs`: add `FileSystem::supports_named_streams(&Path)`. RealFs on Windows uses `GetVolumeInformationW`'s `FILE_NAMED_STREAMS`, and other platforms return `Ok(false)`. FakeFs gets a toggle.
- [x] 5.3 `fs_ops/worker.rs` `copy_one_file`: push a warning for a failed `list_streams`, each failed `copy_stream_chunked`, a failed `set_modified`, and a failed `set_readonly`. Still return `Done`. When the destination lacks stream support, skip stream copies and add one job-level warning with the file count. Append warnings to the outcome's `skipped` vector.
- [x] 5.4 Engine tests with FakeFs fault injection:
  - an injected `set_modified` failure gives `Completed` with one `Warning`, the file counted in `files_done`, and the destination content written;
  - an injected `copy_stream_chunked` failure gives a warning naming the stream;
  - a `list_streams` failure gives one warning;
  - `supports_named_streams == false` with three ADS-bearing files gives exactly one job-level warning naming 3 files;
  - a clean copy gives no warnings and an empty `skipped`.
- [x] 5.5 `views/skipped_summary.rs`: list skipped rows, then a `Warnings` group. Title the dialog `Skipped N item(s)`, `N warning(s)`, or `Skipped N item(s), M warning(s)`. Add insta snapshots for warnings only and for mixed. Existing skipped-only snapshots stay unchanged.
- [x] 5.6 Reducer tests: a `JobDone(Completed)` whose `skipped` holds only warnings moves to `FileOpSummary` and still emits the re-read listings; a `JobDone(Completed)` with an empty `skipped` still returns to `Panels` with no summary (operation-dialogs "End-of-job skipped-files summary").

## 6. Docs

- [x] 6.1 README: a "Diagnostics" section covering `--log`, `FILECOMMAND_LOG`, what is recorded, and where to attach the file in a bug report. Add `--log` to the flag table.

## 7. Verification

- [x] 7.1 `cargo test --workspace` passes.
- [x] 7.2 `cargo clippy --workspace --all-targets` reports no new warnings.
- [x] 7.3 `openspec validate error-diagnostics --strict` is clean.
- [ ] 7.4 Manual (Windows):
  - Run `filecommand --log %TEMP%\fc.log`, make the profile directory read-only, and change the theme. A `persist` line appears.
  - Copy a downloaded file to a FAT32 stick. The summary shows one stream warning.
  - Navigate to `C:\System Volume Information`. The status line reads `Cannot read System Volume Information: Access is denied. (os error 5)`.
