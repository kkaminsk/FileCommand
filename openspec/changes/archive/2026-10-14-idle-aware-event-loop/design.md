# Design: idle-aware-event-loop

## Context

Verified on `build/open-proposals-2026-10` (commit `1bada6f`):

- **Loop shape** (`app.rs:349-451`). Each iteration: `drain_events` (its `applied` flag seeds `dirty`), then one terminal event — a stashed `pending_event`, or `event::poll(POLL_INTERVAL = 33 ms)` (`app.rs:33`). A pressed key is mapped against the current state (`map_key`, or `map_viewer_key` + `resolve_viewer_navigation`, or `map_editor_key`) and applied; `dirty = true`. A resize is applied; `dirty = true`. A mouse event drains all queued mouse events into one batch (`app.rs:409-431`). Any other event (key release, focus, paste) is ignored. A poll timeout applies `Command::Tick(clock.now_ms())` and sets `dirty = true` unconditionally (`app.rs:433-440`). If `dirty`, it drains again and draws (`app.rs:443-450`).
- **A lost redraw.** When a key maps to no command the branch does `continue` (`app.rs:397`), skipping the draw even if the drain at the top of that iteration applied worker results; the next iteration recomputes `dirty` from a fresh (empty) drain. Today the next 33 ms Tick repaints; with an idle loop the screen could stay stale indefinitely.
- **What `Tick` drives** (`update.rs:1227-1236`, `1405-1410`, `1738`): it sets `State::clock_ms`, expires clipboard feedback whose `expires_at_ms` has passed, and ends the splash after `SPLASH_MIN_HOLD_MS = 800`. Animations are pure functions of `clock_ms − stamp`. The animation clock/stamps foundation (`anim.rs`/`stamps.rs`) was verified on that branch but is **not on the current branch** — it comes from the still-open `ui-animations` change, and this change assumes `ui-animations` has landed. In that model `stamps::Snapshot::apply` stamps opened or changed surfaces with the current `state.clock_ms` after every `update`. Durations: splash entrance 760 ms, panel wipe 150 ms, menu-bar sweep 100 ms, pop-up unfold 100 ms, highlight settle 50 ms, dialog iris 100 ms.
- **Other `clock_ms` readers:** mouse double-click detection (`input/mouse.rs` `MouseTracker::last_click`, compared against `state.clock_ms` within ~400 ms), frecency visits recorded by `begin_listing`, and fuzzy-jump ranking at render time (`views/mod.rs:474`).
- **Header clock.** `draw` reads `RealWallClock::now_local()` (hour, minute) on every frame (`app.rs:1219`); it is not part of `State`.
- **Worker senders.** Only worker threads post to `rt.tx` asynchronously (`spawn_listing`, `spawn_job`, `spawn_info_query`, `spawn_git_info_query`, `spawn_drive_label`, `spawn_tree_expand`, `spawn_find_subtree`, `spawn_viewer_search`); `run_effects` and `reload_user_menu` send synchronously from the loop thread, and those messages are drained before the next draw. The timed-out inner git thread sends to a private channel only.
- **Terminal modes.** `terminal.rs` `enter_commands` writes `EnterAlternateScreen, Hide[, EnableMouseCapture]` (`terminal.rs:147-153`); `suspend_with`/`resume_with` order raw mode, mouse capture, and the alternate screen through injected closures (unit-tested); `restore_terminal` (Drop and panic hook) unconditionally disables mouse capture. Bracketed paste is never enabled.
- **crossterm 0.28.1** (`Cargo.lock`; checked in the registry source): `Event::Paste` is produced only by the Unix input parser (`event/sys/unix/parse.rs`); the Windows event source (`event/sys/windows/parse.rs`) reads console input records and never emits it. `EnableBracketedPaste` returns `Unsupported` only on the legacy-console path (`execute_winapi`); on an ANSI-capable Windows console, which is what `execute!` uses whenever `supports_ansi()` is true (Windows 10+ conhost, Windows Terminal), it writes `CSI ?2004h` like any other platform. The terminal may then wrap pastes in `ESC[200~`/`ESC[201~`, which the Windows reader can only deliver as ordinary key events. `DisableBracketedPaste` writes `CSI ?2004l` or, on the legacy path, is an `Ok(())` no-op. On Windows a paste therefore always arrives as individual key events — so the audit's "enable bracketed paste and map `Event::Paste`" fixes P-06's paste case only off Windows; the key-burst coalescing below is what fixes it on Windows, and the mode must not be enabled there.
- **Paste plumbing.** As verified on that branch, Ctrl+V / Shift+Ins map to `Command::RequestPaste` wherever a single-line text field owns the keyboard (dialog fields, and Ctrl+V over the panels for the command line; `input/mod.rs:364`, `:762-763`); `Effect::ReadClipboardText` flattens line breaks to a space and sends `Command::TextEdit(TextEditOp::Paste(text))` (`app.rs:1015-1030`), which the reducer routes to whichever field owns the keyboard. `TextEditOp::Paste` is **not on the current branch** — it comes from the still-open `line-editing-in-text-fields` change, and this change assumes it has landed (tasks.md's landing note already requires archiving that change first). The built-in editor maps Ctrl+V to `EditorPaste` (its own buffer) and typed characters to `EditorChar`/`EditorNewline`; its search/replace prompts and the viewer's search prompt take characters one at a time.
- **Progress.** `copy_one_file` (`fs_ops/worker.rs:268-286`) clones `ProgressInfo` (including the `current_file` `OsString`) and calls `sink.progress` on every 256 KiB chunk; start-of-job and per-file emitters exist for mkdir, copy, move, delete, and rename (`fs_ops/worker.rs:77, 136, 169, 334, 411, 444, 495, 525, 570`). `ChannelSink::progress` (`worker.rs:230-232`) forwards each one as `Command::JobProgress`, a full reducer pass including the stamps snapshot (`update.rs:1142-1144`).

## Goals / Non-Goals

**Goals:**

- Zero redraws while nothing visible can change; unchanged responsiveness to input and worker results.
- Animations, clipboard-feedback expiry, splash hold, and the minute clock keep their current timing.
- A burst of queued input costs one frame, with input order and per-key routing exactly as today.
- Off Windows, a terminal paste is one insertion into the focused field, never a stream of keystrokes that can trigger commands.
- Job progress traffic bounded at ~20 Hz without ever showing stale figures at a decision point or at completion.

**Non-Goals:**

- A unified input+worker queue or a dedicated input thread (would require pausing the reader around every suspended child process).
- Making `Event::Paste` work on Windows (needs a different input backend), changing what Ctrl+V/Shift+Ins do, or making typed multi-line text in the command line behave differently on Windows.
- The budgeted worker drain itself (`streaming-listing-performance`), moving copies to `CopyFileExW` (`durable-file-copy`), Ctrl+C handling and quit/join behavior (`process-lifecycle-hardening`), resize semantics (`resize-preserves-session-state`).

## Decisions

### D1: Redraw on change or deadline; persistent "needs draw"

`dirty` becomes `needs_draw`, declared outside the loop and cleared only by a draw, so a drained worker result followed by an unmapped key still repaints (fixes the lost redraw in Context). It is set by applied worker results, applied input, resize, and by a deadline Tick (D2). A poll timeout with no due deadline applies nothing and draws nothing.

### D2: Deadlines come from a pure core query plus the wall clock

Core gets `anim::next_wake_ms(&State) -> Option<u64>`: the earliest reducer-clock time at which `update(Tick)` or a render would differ from now — `clock_ms + FRAME_MS` (33 ms) while any stamped surface is still inside its animation duration (only when `state.animations` is on), the splash hold end in `UiPhase::Splash`, and the earliest clipboard-feedback `expires_at_ms`. The TUI adds the next wall-clock minute boundary from `clock.rs` (`60_000 − unix_ms % 60_000`; local offsets are whole minutes). It sleeps until the earliest of them; when one is due it applies `Tick(now)` and sets `needs_draw`.

*Alternatives:* have `update` report "did this Tick change anything" — the reducer cannot know whether a *render* changes (animations are render-only), and the minute clock is TUI-side; rejected. Keep ticking at 30 Hz but skip identical frames by diffing buffers — still renders 30×/s, the cost P-05 is about; rejected.

### D3: Wake for workers only while workers exist

crossterm's `event::poll` cannot also wait on the `mpsc` channel, so while any worker that can post results is alive the poll timeout is capped at `POLL_INTERVAL` (33 ms) — today's latency for worker results. A `WorkerGauge` (`Arc<AtomicUsize>`) counts them: every `spawn_*` in `worker.rs` moves a `WorkerToken` into its thread, which decrements on drop (normal exit or panic) after the thread's last send. With no live workers the loop sleeps until the next deadline (at most the next minute boundary). Race: a worker can send and then drop its token between the loop's drain and its gauge read, so after reading zero the loop drains once more before sleeping; the send happens-before the decrement (release/acquire), so that drain sees the message.

*Alternatives:* a dedicated input thread forwarding crossterm events into the worker channel, letting the loop block on one `recv_timeout` — clean, but that thread would keep reading console input while a shell command, external editor, or scrollback view owns the terminal, stealing the child's keystrokes unless paused by a handshake around every suspend; rejected for this change. A fixed longer idle poll (e.g. 250 ms) — adds up to 250 ms to every worker result; rejected.

### D4: Refresh the reducer clock before input

Before applying the first input event of a batch (key burst, mouse batch, paste, resize), the loop applies `Tick(now)` when the last delivered tick is older than `POLL_INTERVAL`. This keeps today's invariant that `clock_ms` is at most ~33 ms stale whenever input is applied, so a menu opened after a minute of idling is stamped with the real time and unfolds over its full 100 ms; double-clicks separated by idle time are not mistaken for one; frecency visits get real timestamps. It applies no transition early: a key during the splash hold still dismisses the splash and is consumed, exactly as before, because the splash-end deadline already fires a Tick at 800 ms.

### D5: Key bursts — drain ready input, then draw once

After applying a pressed key, the loop keeps reading with `event::poll(Duration::ZERO)` while the burst budget (64 events or 8 ms) lasts: each pressed key is mapped against the **current** state through the same mapping function as the first key (factored out of the loop body) and applied; key releases and focus events are skipped; a resize is applied; a paste is handled per D6; a mouse event is stashed in `pending_event` and ends the burst (the mouse-batch path takes it next iteration). A `Quit` effect ends the loop immediately as today. Then one draw. Mapping each key after the previous one is applied preserves every state-dependent routing rule (Enter opening a dialog routes the next characters into the dialog).

### D6: Bracketed paste where the terminal reports it; one paste into the focused field

`enter_commands`, `resume_with`, and `suspend_with` gain an enable/disable step for bracketed paste, `cfg(not(windows))` only (see Context: crossterm cannot report pastes on Windows, and on an ANSI-capable Windows console enabling the mode does write `CSI ?2004h`, so the terminal could wrap pastes in delimiters the Windows reader would deliver as stray keys). Order: enabled after the alternate screen is entered (beside mouse capture), disabled before it is left; `restore_terminal` disables it unconditionally (a no-op where unsupported). The closure-injected order tests are extended.

`Event::Paste(text)` goes through `input::map_paste(text, &state)`:

- If Ctrl+V in the current context maps to `Command::RequestPaste` — i.e. a single-line text field owns the keyboard (a dialog field, or the command line over the panels) — produce `Command::TextEdit(TextEditOp::Paste(flatten_line_breaks(text)))`, the exact command Ctrl+V's clipboard read produces; `flatten_line_breaks` is shared with `Effect::ReadClipboardText`. Deriving the target from the Ctrl+V mapping means no second routing table.
- In the built-in editor's text: the commands the characters would produce if typed (`EditorChar`, `EditorNewline` for each line break), applied as one batch.
- In the editor's search/replace prompts and the viewer's search prompt: the per-character commands with line breaks flattened to spaces (so a pasted line break never confirms the prompt).
- Anywhere else (menus, confirmation and conflict dialogs, help): ignored — a pasted `y` can no longer answer a confirmation.

### D7: Progress throttle at the sink, with flush points

`fs_ops::job::ProgressThrottle` is pure: `offer(info, now) -> Option<ProgressInfo>` returns `info` if at least 50 ms passed since the last delivered update (or none was delivered yet), otherwise keeps it as `pending` and returns `None`; `flush() -> Option<ProgressInfo>` returns the pending update. `ChannelSink` owns one and calls `Instant::now()`; it flushes before sending `JobConflict`, `JobError`, and `JobDone`. Placing it at the sink rather than inside `copy_one_file` throttles every emitter — per-chunk copy progress, per-file delete/move progress, and the `CopyFileExW` progress routine `durable-file-copy` introduces — in one place, and keeps `run_job` and its `FakeFs` tests unthrottled and deterministic. The per-chunk `ProgressInfo` clone remains; it is negligible next to a 256 KiB read and write. Cancellation is unaffected: the worker still calls `is_cancelled` at every file and chunk boundary.

*Alternative:* throttle in `copy_one_file` with an `Instant` check (the audit's suggestion) — covers only the chunk path, not 100,000-file deletes, and puts wall-clock reads into the engine its tests drive; rejected.

## Risks / Trade-offs

- [A future deadline source is forgotten, so something stops updating while idle] → `next_wake_ms` is the single place that enumerates timed state; a unit test opens each animated surface and asserts a wake inside its duration; the minute clock and clipboard expiry have their own tests.
- [Worker results arrive up to 33 ms late while workers live — unchanged from today] → accepted; zero-latency would need D3's rejected input thread.
- [A burst defers drawing by up to 8 ms] → bounded; key-repeat bursts become smoother overall because each key no longer costs a frame.
- [Ignoring pastes outside text contexts changes behavior off Windows] → today the same paste would fire arbitrary hotkeys and dialog answers; ignoring is the safe reading of "a paste is text".
- [Windows gets no single-paste insert] → documented; key-burst coalescing gives it one redraw per burst.

## Migration Plan

None — no persisted data or configuration changes.

## Open Questions

None.
