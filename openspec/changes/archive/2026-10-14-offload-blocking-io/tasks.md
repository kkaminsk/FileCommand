# Tasks: offload-blocking-io

## 1. Core: command-line probe (`crates/filecommand-core/src/update.rs`)

- [x] 1.1 Add `Effect::ProbePath { request: u64, path: PathBuf }`, `Command::PathProbed { request: u64, kind: ProbeKind }`, and `ProbeKind { Dir, File, Missing }`. Add `State::command_probe: Option<PendingProbe>`, holding `{ request, side, cwd, path, intent: Cd { text } | Delete { want_dir } }` (design D1).
- [x] 1.2 Strip the `std::fs::metadata` fallback from `probe_command_line_target`, so it becomes a listed-entry lookup only that returns `None` when the target is not listed (application-shell "The reducer performs no filesystem I/O").
- [x] 1.3 `dispatch_cd` and `dispatch_delete_builtin`:
  - on a listed-entry hit, behave exactly as today;
  - otherwise mint a request id, record the pending probe, set the panel's `Checking <path>…` note, and return `ProbePath`.

  `.` and `..` rejection in `del`/`rmdir` stays before any lookup (command-line "cd navigates…", "del and rmdir route…").
- [x] 1.4 Handle `PathProbed` per design D1:
  - apply only when the request matches, the panel `cwd` is unchanged, and (for `Delete`) the phase is `Panels` with no modal field set;
  - reuse today's navigation, history push and `PersistHistory`, the delete-dialog entry, and the error messages;
  - always clear the pending entry and the note.

## 2. Core: opens, save, launch

- [x] 2.1 Add `PanelState::pending_note: Option<String>`. Setting it clears that panel's `last_error`. Clear it when a reply is applied or discarded and in `begin_new_listing` (design D3).
- [x] 2.2 Add `request: u64` to `Effect::OpenViewer`/`OpenEditor` and to the replies `ViewerOpened`, `ViewerOpenFailed`, `EditorOpened`, `EditorTooLarge`, and `EditorOpenFailed`, plus `EditorNotEditable`, the redirect reply added by `editor-encoding-safety`. Add `State::pending_open`. `handle_request_viewer`, `handle_request_editor`, and the user-menu-file editor path (`update.rs:3692`) set the `Opening <name>…` note. Apply a reply only when the request matches, the phase is `Panels`, and no modal field is set; otherwise discard it. Failure messages go to the recorded side (design D4).
- [x] 2.3 `EditorState::save_pending`:
  - set it when `EditorSave` or `EditorConfirmQuitSave` emits `SaveEditor`;
  - while it is set, `handle_editor` ignores everything except `EditorSaved` and `EditorSaveFailed`;
  - both replies clear it.

  (design D6; application-shell "Slow storage never blocks the event loop")
- [x] 2.4 Add `side: PanelSide` to `Effect::OpenWithAssociation` and `Command::OpenWithAssociationFailed`. The failure lands on that side's `last_error` (design D7).

## 3. TUI workers and runtime (`crates/filecommand-tui/src/worker.rs`, `app.rs`)

- [x] 3.1 `worker::spawn_path_probe(request, path, tx)`: `std::fs::metadata(to_extended_path(&path))` mapped to `ProbeKind` (design D2).
- [x] 3.2 `worker::spawn_viewer_open(request, path, handoff_tx, tx)`: `ByteSource::open`. On success, send the source on the hand-off channel, then `ViewerOpened`. On failure, send `ViewerOpenFailed` (design D4, D5).
- [x] 3.3 `Runtime::viewer_handoff`. In `apply`, after a `ViewerOpened` is processed, drain the hand-off channel: keep the matching source in `rt.viewer_source` only if the phase is `Viewer`, and drop the rest (design D4).
- [x] 3.4 `worker::spawn_editor_open` and `worker::spawn_editor_save`, wrapping `EditorState::open` and `EditorState::save` with the same replies as today plus the request id (design D4, D6).
- [x] 3.5 `worker::spawn_open_with_association(side, path, cwd, tx)`. On Windows it brackets `open_with_association` with `CoInitializeEx(null, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)` and a conditional `CoUninitialize`. Add the `Win32_System_Com` feature to `windows-sys` in `Cargo.toml` (design D7).
- [x] 3.6 `run_effects`: dispatch `ProbePath`, `OpenViewer`, `OpenEditor`, `SaveEditor`, and `OpenWithAssociation` to those workers. No filesystem or shell call remains in these arms.
- [x] 3.7 Add `ConfigWriter` (design D9): spawned in `run` only when `persist_enabled`; `submit(ConfigWrite::Theme(name) | ConfigWrite::PanelSplit(percent))` stores the newest value per key and queues the key once (FIFO); one thread takes keys in order and calls the existing `config::save_theme_atomic`/`save_panel_split_atomic` (which `config-parsing-robustness` routes through the durable-replace helper; do not bypass it); failures go to `diag::log` when `error-diagnostics` has landed. `Drop` sends `Flush`, the thread writes every pending key, and the drop joins with a bounded wait of about 2 s (application-shell "Slow storage never blocks the event loop").
- [x] 3.8 `run_effects`: `PersistTheme` and `PersistPanelSplit` call `rt.config_writer.submit(..)` and return at once. `PersistHistory` is left to `history-persistence-hardening`.

## 4. TUI rendering

- [x] 4.1 `views/panel.rs` `render_bottom_border`: show `pending_note` in the normal ministatus style, below `status_override` and above `last_error` (design D3).
- [x] 4.2 `views/editor.rs`: while `save_pending` is set, show a one-row `Saving…` notice on the last body row, above the F-key bar, in the row `editor-encoding-safety` uses for its save-failure overlay. Leave the header and F-key bar unchanged (builtin-editor "Full-screen editor chrome").
- [x] 4.3 Snapshots: a panel with `Checking \\nas\share…`, a panel with `Opening notes.txt…`, and the editor with the `Saving…` notice row (header and F-key bar unchanged).

## 5. Tests

- [x] 5.1 Core:
  - `cd` to an unlisted target emits `ProbePath` with the panel unchanged, so the result does not depend on the test machine's disk;
  - `PathProbed { Dir }` navigates and records history;
  - `{ File }` gives "is not a directory";
  - `{ Missing }` gives "not found" with no `StartListing`.

  Migrate the existing tests (`update/tests.rs` around `:1976-2050`) to drive the reply.
- [x] 5.2 Core:
  - `del`/`rmdir` unlisted target, then `PathProbed`, opens the delete dialog with a type match, or gives the matching error;
  - a reply after the F9 menu opened opens no dialog;
  - a reply after a newer probe is ignored;
  - a reply after the panel navigated is ignored.
- [x] 5.3 Core: the listed-entry fast path emits no `ProbePath` (`cd sub` with `sub` listed starts the listing at once).
- [x] 5.4 Core:
  - open replies with a stale request id are discarded;
  - a reply arriving with `menu` open is discarded;
  - `EditorTooLarge` re-requests the viewer with a fresh id;
  - a failure message lands on the recorded side after a Tab.
- [x] 5.5 Core: while `save_pending` is set, `EditorChar`, `EditorMove`, `EditorSave`, and `EditorRequestQuit` leave the editor unchanged and emit nothing. `EditorSaved` clears the flag and the modified state. `EditorSaveFailed` clears the flag and sets `save_error`.
- [x] 5.6 TUI:
  - `spawn_path_probe` reports `Dir`, `File`, and `Missing` for a temp directory, a temp file, and a missing path;
  - on Windows it reports `Dir` for a directory whose absolute path exceeds 260 characters (created with `\\?\`).
- [x] 5.7 TUI: `spawn_viewer_open` delivers the source on the hand-off channel before `ViewerOpened`. `apply` keeps only the matching source and drops a non-matching one. Add a compile-time `fn assert_send<T: Send>()` check for `ByteSource`.
- [x] 5.7a TUI `ConfigWriter` tests with a temp profile directory and an injectable save function:
  - eight split submissions in a row produce a final `panel_split` equal to the last one, with no more writes than submissions;
  - a theme submission followed by a split submission leaves both keys in `config.toml`, and the two saves never run concurrently (a save hook that asserts no overlap);
  - dropping the writer with a pending write completes it before `drop` returns;
  - a save hook that blocks forever makes `drop` return after about 2 s;
  - with `persist_enabled` false, `submit` writes nothing.
- [x] 5.8 TUI (Windows): `spawn_open_with_association` on a missing file replies `OpenWithAssociationFailed` with the original side. A unit test of the COM bracket helper checks that it uninitializes only after a successful initialization.

## 6. Verification

- [x] 6.1 `cargo test --workspace` passes.
- [x] 6.2 `cargo clippy --workspace --all-targets` reports no new warnings.
- [x] 6.3 `openspec validate offload-blocking-io --strict` is clean.
- [ ] 6.4 Manual (Windows):
  - `cd \\10.255.255.1\x`: the UI stays live, `Checking …` shows, and "not found" appears after the timeout.
  - F3, F4, and F2 on a file on an SMB share with the server paused: the UI stays live.
  - Open on a `.pdf` still launches the reader.
