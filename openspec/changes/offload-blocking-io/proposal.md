# Change: offload-blocking-io

## Why

Audit findings **R-09 (High)** and **P-09 (Low)**: filesystem and shell I/O runs on the event-loop thread. On slow, removable, or unreachable storage, that freezes input and redraw.

- **R-09.** `core::update` itself performs filesystem I/O. `probe_command_line_target` (`update.rs:1929-1941`), called from `dispatch_cd` (`:1856`) and `dispatch_delete_builtin` (`:1902`), falls back to `std::fs::metadata(path)` inside the reducer whenever the target is not a listed entry of the active panel. This breaks the application-shell "Pure core update function" contract ("performs no I/O"). `cd \\offline-server\share` then blocks the event loop for the SMB timeout, typically 20–60 s, with no redraw and no input. The same call skips the `\\?\` long-path abstraction, so a `cd`, `del`, or `rmdir` target longer than 260 characters is reported as "not found".
- **P-09.** `run_effects` runs several file operations synchronously on the UI thread:
  - `ByteSource::open` for F3 (`app.rs:926`);
  - `EditorState::open` for F4, a read of up to 10 MB (`app.rs:967`);
  - `editor.save()` for F2 in the editor (`app.rs:985-994`);
  - `ShellExecuteW` for Open with the associated application (`app.rs:878-886`, `:1087`), which can block for seconds in slow handlers and is called without `CoInitializeEx`, though MSDN asks for it because shell handlers may use COM.

  - the `config.toml` rewrites for `Effect::PersistTheme` and `Effect::PersistPanelSplit` (`app.rs:900-915`): each reads the whole file, rewrites one line, writes a temporary file, and renames it, on every theme apply and every Ctrl+Left/Ctrl+Right split step.

  On a network share or removable media, F3, F4, F2, and Open can each freeze the UI for as long as the I/O takes, and so can a theme or split change when the profile directory sits on a slow or redirected volume.

## What Changes

- The reducer no longer touches the filesystem. `cd`, `del`, and `rmdir` targets that are already listed in the active panel are still answered immediately from that listing. Any other target is checked asynchronously: the reducer emits a path-probe effect carrying a fresh request id, and completes the command when the matching probe reply arrives.
  - A reply that has been superseded is discarded: a newer check was started, the panel has navigated since, or (for `del`/`rmdir`) the panels no longer own input.
  - While a check is outstanding, the panel shows `Checking <path>…`.
  - The probe runs through the `\\?\` long-path abstraction.
- Viewer open, editor open, editor save, and launching the associated application all run on worker threads.
  - While an open is outstanding, the panel shows `Opening <name>…`. A newer F3 or F4 supersedes it. A late reply is discarded once the user has left the panels or opened a dialog, menu, or overlay.
  - While a save is outstanding, the editor shows a `Saving…` notice on its last body row, above the F-key bar, and edits are not accepted. The header and F-key bar that builtin-editor "Full-screen editor chrome" specifies are unchanged.
  - On Windows, the launch worker initializes COM (apartment-threaded) around `ShellExecuteW`. A launch failure lands on the panel the Open action targeted.
- The theme and panel-split `config.toml` writes move to one background configuration writer:
  - writes are applied one at a time in the order requested, so two rewrites of `config.toml` never interleave;
  - a newer value for the same setting replaces an older one that has not been written yet (last write wins);
  - on quit the writer finishes its outstanding writes before the process exits, waiting at most about 2 seconds;
  - each write keeps the durable temp-file, flush, and replace sequence that `config-parsing-robustness` specifies, and a failure still never interrupts the session.

  `PersistHistory` (`history.json`) is not part of this; `history-persistence-hardening` gives it its own background writer.
- `cd`, `del`, and `rmdir` keep their observable outcomes: they navigate, open the delete-confirmation dialog, or reject with the same messages. Only the timing of the outcome changes, plus the pending status text.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `application-shell`: ADDED "The reducer performs no filesystem I/O", which makes the existing purity rule testable and adds the request-id/stale-reply contract. ADDED "Slow storage never blocks the event loop", which covers the viewer and editor open, editor save, the command-line path check, the associated-application launch with COM initialized, and the `config.toml` writes for the applied theme and panel split (ordered, last write wins, completed before exit within a bound). "Pure core update function" is left unchanged.
- `command-line`: MODIFIED "cd navigates the active panel or rejects a nonexistent target" and MODIFIED "del and rmdir route into the existing delete-confirmation flow". Both gain the asynchronous long-path probe with the listed-entry fast path, the pending status, and stale-reply discarding. Their resolution clause now defers to `relative-destination-resolution`'s ADDED "cd, del and rmdir targets resolve to a normalized absolute path" instead of "exactly as today". All existing scenarios are kept verbatim.

## Impact

- `crates/filecommand-core/src/update.rs`:
  - `probe_command_line_target` (`:1929-1941`) loses its `std::fs::metadata` fallback and becomes a listed-entry lookup only.
  - `dispatch_cd` (`:1848-1872`) and `dispatch_delete_builtin` (`:1884-1919`) gain pending-probe state and emit `Effect::ProbePath { request, path }`.
  - New `Command::PathProbed { request, kind }` handling.
  - `Effect::OpenViewer` and `Effect::OpenEditor` gain a request id, and their replies (`ViewerOpened`/`ViewerOpenFailed`, `EditorOpened`/`EditorTooLarge`/`EditorOpenFailed`) echo it (`:646-732`, `:1622-1659`).
  - `handle_editor` (`:2332-2453`) gains a save-in-flight state.
  - `Effect::OpenWithAssociation` and its failure reply carry the panel side.
- `crates/filecommand-core/src/panel.rs`: a non-error pending note on the mini-status line.
- `crates/filecommand-tui/src/app.rs`, `run_effects` (`:843-1035`):
  - `ProbePath`, `OpenViewer`, `OpenEditor`, `SaveEditor`, and `OpenWithAssociation` dispatch to workers.
  - `PersistTheme` and `PersistPanelSplit` (`:900-915`) are submitted to a new `ConfigWriter` owned by `Runtime`, which is flushed and joined (bounded) on every exit path from `run`.
  - `Runtime` gains a hand-off slot for the `ByteSource` a viewer-open worker produces.
  - `open_with_association` (`:1064-1144`) runs inside a COM-initialized worker on Windows.
- `crates/filecommand-tui/src/worker.rs`: new `spawn_path_probe`, `spawn_viewer_open`, `spawn_editor_open`, `spawn_editor_save`, and `spawn_open_with_association`.
- `crates/filecommand-tui/Cargo.toml`: the `windows-sys` features gain `Win32_System_Com`.
- `crates/filecommand-tui/src/views/`: the panel mini-status shows the pending note, and the editor shows the `Saving…` notice row.
- Tests:
  - core reducer tests for probe emission, reply application, stale discarding, the listed-entry fast path, and save-in-flight gating;
  - TUI worker tests for the probe (including a >260-character path on Windows) and for viewer and editor open hand-off;
  - snapshots of the pending note and of the `Saving…` notice row.

**Audit findings:** R-09, P-09

**Coordination:**
- `editor-encoding-safety` (R-02) must land first. It rewrites `EditorState::open`/`save`: encoding detection, redirecting non-UTF-8 files to the viewer, and atomic save. This change only moves those calls onto a worker and tags their replies with the request id, including its new `EditorNotEditable` redirect reply. It also renders `save_error` on the row above the editor's F-key bar, which is where this change shows `Saving…`.
- `relative-destination-resolution` (R-06) **lands first**. It makes `resolve_cd_target` return an absolute, lexically normalized path and ADDS command-line "cd, del and rmdir targets resolve to a normalized absolute path", which also says the existence check uses the resolved path. This change rebases onto it: the listed-entry fast path and the asynchronous probe both take that resolved path, and the MODIFIED cd and del/rmdir descriptions defer to that requirement for resolution instead of saying "exactly as today".
- `history-persistence-hardening` (P-08/R-13/R-15) owns `PersistHistory` only: its background `history.json` writer, debounce, and bounded final flush. This change owns every other `Persist*` effect (today `PersistTheme` and `PersistPanelSplit`) and follows the same shape: one writer thread spawned when persistence is enabled, a bounded flush at exit. The two writers handle different files and are independent; whichever change lands second may fold them into one persistence thread.
- `config-parsing-robustness` (R-15/R-16/B-05) owns how a `config.toml` write is performed: its ADDED configuration-files "Persisted configuration writes are durable" (uniquely named temporary file, flushed with `sync_all`, then replaced; target untouched and temporary file removed on failure), implemented by routing `save_theme_atomic`/`save_panel_split_atomic` through the shared durable-replace helper (introduced by `editor-encoding-safety`, or by `config-parsing-robustness` if it lands first). This change only moves those calls onto the writer thread and must not bypass the helper. Its table-aware rewrite ("Configuration rewrites keep settings in the top-level table") is likewise unchanged by being called from the writer.
- `error-diagnostics` (B-06) logs persistence failures. The `persist` log lines move with the writes onto the writer thread; `diag::log` is already safe to call from any thread.
- `open-with-associated-app` (in-flight) introduced `ShellExecuteW`. This change adds only application-shell requirements and does not modify `file-action-menu`. The failure still lands on the status line of the panel the Open action was invoked on.
- `listing-request-guard` (R-04) uses the same request-id pattern (`State::next_request_id`) for listings.
- `viewer-mapping-safety` (R-20) changes the backend `ByteSource::open` uses. The worker calls whichever backend exists.
- `process-lifecycle-hardening` (R-12) adds `catch_unwind` around worker bodies. The new workers must follow the same pattern; whichever change lands second wraps them.
- `resize-preserves-session-state` (R-03): replies that arrive while the terminal is undersized are applied, because being undersized is not "leaving the panels".
