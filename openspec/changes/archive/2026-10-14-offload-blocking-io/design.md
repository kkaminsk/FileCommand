# Design: offload-blocking-io

## Context

Verified on `build/open-proposals-2026-10` (`1bada6f`).

- **The reducer queries the filesystem.** `probe_command_line_target(state, side, path)` (`update.rs:1929-1941`) first looks for `path` among the active panel's listed entries when `path` is a direct child of the panel's `cwd`. If it is not there, it calls `std::fs::metadata(path).ok()?` and returns `Some((is_dir, name))`, or `None` for any error.
  - It is the only filesystem call reachable from `core::update`. A search of `update.rs` finds no other `std::fs`, `metadata`, `read_dir`, or `canonicalize`. `config.rs` does do I/O, but only the TUI calls it, from `run_effects`.
  - `dispatch_cd` (`:1848-1872`) navigates on `Some((true, _))`, after pushing the line to history and emitting `PersistHistory`. It reports "`<path>` is not a directory" on `Some((false, _))` and "`<path>` not found" on `None`.
  - `dispatch_delete_builtin` (`:1884-1919`) rejects `.` and `..` before any lookup, then opens `enter_delete_confirm_for_sources` on a type match, or reports "is a directory", "is not a directory", or "not found".
  - In both, `run_command_line` has already cleared the command line (`:1816`).
  - The `metadata` call does not go through `fs_ops::path::to_extended_path`, so a target longer than `MAX_PATH` fails and is reported as not found.
- **`run_effects` opens, saves, and launches on the event-loop thread** (`app.rs:843`):
  - `Effect::OpenViewer` (`:926-939`) runs `ByteSource::open`, which is `File::open`, then `metadata`, then `Mmap::map`. On success it stores the source in `rt.viewer_source` and sends `ViewerOpened { path, file_len }`. On failure it sends `ViewerOpenFailed { message }`.
  - `Effect::OpenEditor` (`:967-982`) runs `EditorState::open`, a whole-file read capped at 10 MB, and replies `EditorOpened`, `EditorTooLarge`, or `EditorOpenFailed`.
  - `Effect::SaveEditor { editor, then_quit }` (`:985-995`) runs `editor.save()` on the boxed clone and replies `EditorSaved { editor, then_quit }` or `EditorSaveFailed`. `handle_editor` replaces its whole `EditorState` with the saved clone on `EditorSaved` (`update.rs:2422-2430`). Any edit made between dispatch and reply would therefore be lost if the save were simply moved to a worker.
  - `Effect::OpenWithAssociation { path, cwd }` (`:878-886`) calls `open_with_association`, which runs `ShellExecuteW` (`:1087`) with no `CoInitializeEx` anywhere in the workspace. On failure it sends `OpenWithAssociationFailed { message }`, which core applies to `state.active`'s `last_error` (`update.rs:1659`).
- **Configuration writes run inline too.** `Effect::PersistTheme(name)` (`app.rs:900-907`, emitted when the theme picker applies a theme, `update.rs:3739`) and `Effect::PersistPanelSplit(percent)` (`app.rs:908-915`, emitted by every split adjust or reset, `update.rs:1666`, `:2047`) call `config::save_theme_atomic` and `config::save_panel_split_atomic` directly in `run_effects`, guarded by `rt.persist_enabled` and with the result discarded. Each is a read-modify-write of the whole `config.toml`: read the file, replace or append one key's line, write the fixed-name `config.toml.tmp`, and rename it over the target (`config.rs:399-415`, `:458-474`). Holding Ctrl+Left issues one such rewrite per step. These are the only `Persist*` effects besides `PersistHistory`.
- **Existing worker pattern.** `worker.rs` spawns one `std::thread` per request and replies with a `Command` over `rt.tx`. Stale replies are filtered in core by a request id minted from `State::next_request_id` (`update.rs:411-414`), for example `ViewerSearchResult { request }` and `InfoResolved { request }`.
- **`ByteSource` is `Send`.** It holds either a `memmap2::Mmap` or a `File` (`viewer/byte_source.rs:17-26`), so it can be created on a worker and moved to the UI thread. `spawn_viewer_search` already reopens files off-thread (`worker.rs:133-145`).
- **Not cited in the audit.** Quick View mode opens and maps the opposite panel's cursor file inside rendering, on every frame (`views/panel.rs:882-883`, `render_quick_view_body`). That is the same class of blocking I/O. It is recorded as a non-goal below.

## Goals / Non-Goals

**Goals:**

- `core::update` performs no I/O, and a unit test can demonstrate it.
- `cd`, `del`, `rmdir`, F3, F4, F2-in-editor, Open, and the theme and panel-split saves never block input or redraw, whatever the storage.
- Configuration writes stay ordered, keep the last value per setting, and are on disk before a normal exit (within a bound).
- Results that a newer action has superseded never land.
- Command-line targets longer than 260 characters work.
- COM is initialized for the shell launch.

**Non-Goals:**

- Moving `PersistHistory` (`history.json`) off the UI thread. That is `history-persistence-hardening`, which adds its own background history writer with debounce and a bounded final flush. Every other `Persist*` effect (today `PersistTheme` and `PersistPanelSplit`) is in scope here (D9).
- How a configuration write is made durable (temporary file, `sync_all`, replace) and how its rewrite keeps keys in the top-level table. Those are `config-parsing-robustness`'s; D9 only changes which thread calls the save functions.
- Per-frame byte reads of an open viewer (`read_range` during render, and the backward and forward scans in `resolve_viewer_navigation`). These are bounded per frame and inherent to the viewer's design. Its backend is `viewer-mapping-safety`'s.
- Quick View's per-frame `ByteSource::open` (`views/panel.rs:883`). This is outside the audit's R-09/P-09 scope and is flagged as a follow-up proposal (cache the preview source per cursor entry and open it on a worker).
- `reload_user_menu` (a local profile-directory read), `EnumerateDrives` (a `GetLogicalDrives` bitmask read), and clipboard effects.
- `RunShellCommand`/`RunExternalEditor`, which are deliberately synchronous suspended runs.
- Cancelling an outstanding probe, open, or save. A worker stuck on an unreachable server finishes when the redirector times out, and its reply is then discarded or applied by the rules below.
- Changing editor encoding or atomic-save semantics. That is `editor-encoding-safety`, which lands first.

## Decisions

### D1: Command-line probe becomes an effect and reply pair

- `probe_command_line_target` keeps only its listed-entry lookup and becomes the fast path. Its `std::fs::metadata` fallback is replaced by a request:
  - `Effect::ProbePath { request: u64, path: PathBuf }`.
  - `Command::PathProbed { request: u64, kind: ProbeKind }`, where `ProbeKind` is `Dir`, `File`, or `Missing`.
- Core keeps `State::command_probe: Option<PendingProbe { request, side, cwd: PathBuf, path, intent }>`. The intent is either `Cd { text }` (for the history push on success) or `Delete { want_dir }`.
- On `PathProbed`, core applies the reply only when all of these hold:
  - `request` matches;
  - `state.panel(side).cwd` still equals the recorded `cwd`;
  - for `Delete`, the phase is `Panels` and no modal field is set. These are the fields `drag_allowed` checks today, plus `quit_confirm`.

  The reply then goes through the same code paths and messages as today's synchronous outcome. In every case the pending entry and the panel's pending note (D3) are cleared.
- A new `cd`, `del`, or `rmdir` replaces `command_probe`, which supersedes the old request. Any navigation of that panel invalidates it through the `cwd` check.

*Alternative considered:* start the listing optimistically for `cd` and let `ListingFailed` produce the "not found" message, as the audit suggests. This was rejected. The command-line spec requires that a nonexistent target leaves `cwd` unchanged and starts no listing read. `del` and `rmdir` need the entry type before any dialog opens, so the probe is needed anyway.

*Alternative considered:* a timeout on the probe. This was rejected. Nothing blocks while it is outstanding, and a newer command or navigation supersedes it.

### D2: Probe semantics on the worker

`worker::spawn_path_probe` runs `std::fs::metadata(fs_ops::path::to_extended_path(&path))` and maps the result:

- `Ok(md)` with `md.is_dir()` gives `Dir`.
- `Ok(_)` gives `File`.
- Any `Err` gives `Missing`, which matches today's `.ok()?`.

Like today's call, it follows links, so a junction to a directory probes as `Dir`. `to_extended_path` adds `\\?\` or `\\?\UNC\` and normalizes `.` and `..` lexically, as every `fs_ops` call does. The on-disk name for the delete dialog stays `path.file_name()`, as today.

### D3: Pending note on the mini-status line

`PanelState` gains `pending_note: Option<String>`.

- It holds `Checking <path>…` while a probe is outstanding and `Opening <name>…` while an open is outstanding (D4).
- Setting it clears that panel's `last_error`, since a new command supersedes an old error.
- It is cleared when the reply is applied or discarded, and when the panel starts a new listing.
- The mini-status shows it in the normal ministatus style, ranked below the existing `status_override` (clipboard feedback, Tree path, Quick View) and above `last_error`. Showing it this way changes no other state.

### D4: Editor and viewer opens on workers, tagged with a request id

- `Effect::OpenViewer` and `Effect::OpenEditor` gain `request: u64`. Core records `State::pending_open: Option<PendingOpen { request, side, kind }>` and sets the panel's pending note.
- `ViewerOpened`, `ViewerOpenFailed`, `EditorOpened`, `EditorTooLarge`, and `EditorOpenFailed` echo `request`, as does `EditorNotEditable { path, reason }`, the redirect reply `editor-encoding-safety` adds for non-UTF-8 files. `EditorNotEditable` follows the `EditorTooLarge` path below.
- Core applies a reply only when all of these hold:
  - `request == pending_open.request`;
  - the phase is `Panels`;
  - no modal field is set.

  Otherwise it clears the pending state and discards the reply. Failure messages land on the recorded `side`, not on whichever panel is active when the reply arrives. The `EditorTooLarge` path then opens the viewer by minting a fresh request for the same `side`.
- **Viewer hand-off.** `Command` lives in core and derives `Clone`/`PartialEq`/`Eq`, so it cannot carry the source. `Runtime` therefore owns a dedicated `mpsc` pair, `viewer_handoff: (Sender<(u64, PathBuf, ByteSource)>, Receiver<…>)`.
  - `worker::spawn_viewer_open` sends the source on that channel first, and only then sends `Command::ViewerOpened { request, path, file_len }`. That ordering guarantees the source is waiting when the command is applied.
  - In `apply`, after `update` processes `ViewerOpened`, the runtime drains the hand-off channel. It stores the entry whose `request` matches into `rt.viewer_source` if the phase became `Viewer`, and drops every other entry, closing its handle.

*Alternative considered:* `Arc<Mutex<Option<ByteSource>>>` shared with the worker. This was rejected because a channel keeps the ordering explicit and needs no lock on the render path.

### D5: Viewer open moves off the UI thread too

`ByteSource::open` is O(1) in work but not in latency. `File::open` and `Mmap::map` are separate round trips to an SMB server. If the session dropped after the panel was listed, `CreateFileW` blocks for the redirector timeout, the same 20–60 s failure R-09 describes.

*Alternative considered:* keep the open synchronous because the panel was just listed. This was rejected: a listing a few seconds old does not prove the server still answers, and removable or optical media can stall on the open as well. The hand-off (D4) is the only cost, and `spawn_viewer_search` already shows the open is thread-agnostic. "Instant open" (no content scan, first frame reads only the visible window) is unaffected.

### D6: Editor save in flight blocks edits

- `EditorState` gains `save_pending: bool`, set when `EditorSave` or `EditorConfirmQuitSave` emits `Effect::SaveEditor`.
- While it is set, `handle_editor` ignores every command except `EditorSaved` and `EditorSaveFailed`, and the editor shows a one-row `Saving…` notice on the last body row, directly above the F-key bar. That is the row `editor-encoding-safety` uses for its `Save failed: …` overlay (its D6), so the two share one row: while a retry is in flight, `Saving…` covers an earlier failure message (`save_error` itself is left set), and the reply then either clears it (`EditorSaved`) or replaces it with the new failure (`EditorSaveFailed`). The header and F-key bar stay exactly as builtin-editor "Full-screen editor chrome" specifies; putting the notice in the header would contradict that requirement's fixed header layout.
- `EditorSaved` restores the saved state, as today, and clears the flag. With `then_quit`, the editor closes.
- `EditorSaveFailed` clears the flag and sets `save_error`, as today. Today that message is never rendered; `editor-encoding-safety`, which lands first, renders it on the row above the F-key bar.
- The save itself runs in `worker::spawn_editor_save` on the boxed clone that `Effect::SaveEditor` already carries.

*Alternative considered:* let editing continue during the save, using a buffer revision counter, and mark the buffer clean only if it is unchanged since dispatch. This was rejected for now. It interacts with `editor-encoding-safety`'s rewritten save, and a local save completes within a frame, so the gate is only noticeable on slow media, where the user is waiting for the save anyway. It can be revisited later without changing this requirement's observable rule.

### D7: Shell launch worker with COM initialized

- `Effect::OpenWithAssociation` gains `side: PanelSide`, and `OpenWithAssociationFailed` gains `side`, so a late failure lands on the panel the Open action was invoked on.
- `worker::spawn_open_with_association` runs on its own thread. On Windows it:
  - calls `CoInitializeEx(null, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)`, the combination the `ShellExecute` documentation recommends;
  - calls the existing `open_with_association`;
  - calls `CoUninitialize` if initialization returned `S_OK` or `S_FALSE`.
- On other platforms the detached `xdg-open`/`open` spawn runs on the same kind of worker, for uniformity.
- No pending note is shown. The `file-action-menu` contract keeps Open fire-and-forget, and this change does not modify that capability.
- `windows-sys` gains the `Win32_System_Com` feature.

### D8: Request ids and worker robustness

All new requests mint ids through `State::next_request_id`, the same counter that `listing-request-guard` uses for listings. A worker that panics would leave a pending note forever. `process-lifecycle-hardening` wraps worker bodies in `catch_unwind` and sends a failure reply. For the new workers that reply is `PathProbed { kind: Missing }`, `ViewerOpenFailed`, `EditorOpenFailed`, or `EditorSaveFailed`, each with the request id. Whichever change lands second applies the wrapper to these workers.

### D9: One background writer for configuration writes

- `Runtime` gains a `ConfigWriter`, spawned in `run` only when `persist_enabled` is true; otherwise `submit` is a no-op, exactly as today's guard skips the write. It owns one thread and a mailbox of pending writes keyed by setting (`Theme`, `PanelSplit`, and any future non-history `Persist*` effect).
- `run_effects` maps `PersistTheme(name)` and `PersistPanelSplit(percent)` to `rt.config_writer.submit(..)` and returns at once. No filesystem call remains in those arms.
- **Last write wins, in order.** `submit` stores the value under its key, replacing any value for that key the thread has not taken yet, and records the key's position in a FIFO of dirty keys if it is not already queued. The thread takes one key at a time and calls that key's save function with the newest value. So ten quick split steps produce at most a couple of writes, the final value is always the last one written for that key, and the panel-split spec's "rapid repeated adjustments MAY coalesce writes, but the final value SHALL be persisted" holds.
- **One thread, never concurrent.** Both saves are read-modify-write of the same `config.toml`. Running them on separate threads could let one rename overwrite the other's freshly written key. A single writer applies them strictly one after another, and each re-reads the file, so a theme change and a split change made close together both end up on disk.
- **Durability is unchanged.** The thread calls the same `save_theme_atomic`/`save_panel_split_atomic`, which `config-parsing-robustness` routes through the shared durable-replace helper. A failure is logged through `error-diagnostics`' `diag::log` when that change has landed and is otherwise dropped, as today; the in-memory setting stays in effect.
- **Flush before exit.** `ConfigWriter` implements `Drop`: it sends `Flush`, the thread writes every pending key, and the drop joins it with a bounded wait of about 2 seconds. `Runtime` owns the writer, so every exit from `run` flushes, including the early `?` returns and unwinding. A profile directory that stops answering cannot hold the process open, and the durable helper guarantees that an abandoned write leaves `config.toml` either complete-old or complete-new.

*Alternative considered:* a thread per write, like the open workers. This was rejected because concurrent read-modify-writes of one file lose updates, and nothing would order a theme write against a later split write.

*Alternative considered:* one shared writer for `history.json` and `config.toml`. This was not chosen here because `history-persistence-hardening` designs its writer around a debounce and a cross-instance merge that configuration writes do not need. The two threads write different files, so they are independent; whichever change lands second may merge them.

## Risks / Trade-offs

- [Worker threads stuck on an unreachable server accumulate, one per attempt.] → Each attempt is a deliberate user action, and each thread ends when the SMB redirector times out. The same exposure already exists for the git, Info, and drive-label workers.
- [Reducer tests that relied on the synchronous fallback change.] → `cd_to_a_nonexistent_directory_is_rejected_without_navigating` and similar tests now assert a `ProbePath` effect, then feed `PathProbed { kind: Missing }` and assert the same rejection. The end state is unchanged.
- [The F2 gate makes the editor ignore keys for the length of the save.] → The gate is invisible on local disks. On slow media the `Saving…` row says why, and the save is already what the user is waiting on.
- [The hand-off channel can keep a source whose `ViewerOpened` was discarded.] → `apply` drains the channel on every `ViewerOpened` and drops non-matching entries. Every source is sent immediately before its own command, so nothing can linger past that command.
- [A theme or split change made in the last moments before quit could be lost if the writer is still blocked when the 2 s bound expires.] → Only on a profile directory that has stopped answering; the file is then left complete (old or new), never truncated, and the in-memory value was in effect for the session. On a responsive disk the flush completes well within the bound.
- [Quit can now wait on several bounded flushes: the job wait from `process-lifecycle-hardening`, the history writer, and this writer.] → Each is bounded and they only wait when work is outstanding. Signalling every writer to flush before joining any of them lets the waits overlap; whichever change lands second arranges the drop order so.
- [A new `side` field on `OpenWithAssociationFailed` touches code from the in-flight `open-with-associated-app` change.] → The observable rule, "the panel the menu acted on", is the one that change already states, so no `file-action-menu` delta is needed.

## Migration Plan

None. The protocol is in-memory only, and no persisted data changes.

## Open Questions

None.
