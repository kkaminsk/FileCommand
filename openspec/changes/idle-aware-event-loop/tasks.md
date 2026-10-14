# Tasks: idle-aware-event-loop

Landing note: `line-editing-in-text-fields` (in-flight) must be archived before this change is archived, since it creates the `text-field-editing` capability. If `streaming-listing-performance` has landed, keep its drain budget and make its backlog force a zero poll timeout in 2.3.

## 1. Core: wake deadlines and progress throttle (`crates/filecommand-core`)

- [x] 1.1 `anim.rs`: add `next_wake_ms(&State) -> Option<u64>` — `clock_ms + FRAME_MS` while any stamped surface (splash entrance, panel wipe, menu-bar sweep, pop-up unfold, highlight settle, dialog iris, quit dialog) is inside its duration and animations are on; the splash hold end in `UiPhase::Splash`; the earliest clipboard-feedback `expires_at_ms` (design D2)
- [x] 1.2 `anim.rs` tests: for each animated surface, opening it at clock `t` yields a wake before `t + duration` and none once finished; animations off yields no animation wake; splash and clipboard expiry wakes match their deadlines (application-shell "Animations still run after idling", "Clipboard feedback expires without input")
- [x] 1.3 `fs_ops/job.rs`: add `ProgressThrottle` with `offer(info, now) -> Option<ProgressInfo>` (50 ms interval, first offer always delivered) and `flush() -> Option<ProgressInfo>` (design D7)
- [x] 1.4 `fs_ops/job.rs` tests: offers within 50 ms are held and only the latest is flushed; an offer after 50 ms is delivered; 1,000 offers over 1 s deliver ≤ 21 (file-operations "A fast copy does not flood the UI")

## 2. TUI: idle-aware loop (`crates/filecommand-tui/src/app.rs`, `worker.rs`, `clock.rs`)

- [x] 2.1 `worker.rs`: add `WorkerGauge`/`WorkerToken` (RAII decrement on drop, release ordering) and move a token into every `spawn_*` thread; `Runtime` owns the gauge (design D3)
- [x] 2.2 `clock.rs`: add milliseconds until the next wall-clock minute boundary, with a test at known instants (application-shell "Header clock still advances while idle")
- [x] 2.3 `app.rs`: factor the poll-timeout choice into a pure `next_timeout(now, core_wake, minute_wake, workers_alive, backlog)` — zero with a drain backlog, at most `POLL_INTERVAL` while workers are alive, otherwise the earliest deadline — with unit tests (design D2, D3)
- [x] 2.4 `app.rs` loop: replace per-iteration `dirty` with a persistent `needs_draw` cleared only by a draw; on a timeout apply `Tick(now)` and set `needs_draw` only when a deadline is due; after reading a zero gauge, drain once more before a long sleep (application-shell "Idle screen is not redrawn", "Redraw survives an unmapped key"; design D1, D3)
- [x] 2.5 `app.rs`: before applying the first event of an input batch, apply `Tick(now)` when the last tick is older than `POLL_INTERVAL` (application-shell "Clicks separated by idle time are not a double-click"; design D4)
- [x] 2.6 `app.rs`: factor the per-key mapping (`map_key` / `map_viewer_key` + `resolve_viewer_navigation` / `map_editor_key`) into one function; after the first pressed key keep reading with a zero poll up to 64 events or 8 ms, mapping each key against the current state, applying resizes and pastes, skipping releases, stashing a mouse event in `pending_event`, then draw once (application-shell "Bursts of key input render once"; design D5)
- [x] 2.7 Tests: an injectable event source drives the loop helper with a queued burst and asserts one draw and the same final state as one-at-a-time application, including the F7/`new`/Enter routing case (application-shell "Holding PgDn renders one frame per burst", "Routing follows the state each key produces")

## 3. TUI: bracketed paste (`terminal.rs`, `input/mod.rs`, `app.rs`)

- [x] 3.1 `terminal.rs`: enable bracketed paste in `enter_commands` and `resume_with`, disable it in `suspend_with` and `restore_terminal`, `cfg(not(windows))` for the enable; extend the closure-injected order tests (text-field-editing "Child programs get a terminal without bracketed paste"; design D6)
- [x] 3.2 `input/mod.rs`: add `map_paste(text, &state) -> Vec<Command>` — `TextEdit(Paste(flattened))` where Ctrl+V maps to `RequestPaste`; `EditorChar`/`EditorNewline` in the editor text; per-character prompt commands with line breaks flattened in editor/viewer prompts; empty elsewhere (design D6)
- [x] 3.3 Share one `flatten_line_breaks` helper between `map_paste` and `Effect::ReadClipboardText`
- [x] 3.4 `app.rs`: handle `Event::Paste` through `map_paste`, applying the commands as one batch with one draw
- [x] 3.5 `input/tests.rs`: command line over the panels, quick filter active (still the command line, as Ctrl+V), destination field, find-file pattern, editor text with a line break, viewer search prompt, delete confirmation (ignored), menu open (ignored) (text-field-editing "Paste a path into the command line", "Multi-line paste is flattened and not executed", "Paste into a dialog field", "Paste with no focused text input is ignored")

## 4. TUI: progress throttling (`worker.rs`)

- [x] 4.1 `ChannelSink`: own a `ProgressThrottle`; `progress` sends only what `offer(info, Instant::now())` returns; `conflict`, `error`, and `done` send the `flush()`ed update first (file-operations "Progress updates are rate-limited"; design D7)
- [x] 4.2 Tests with a recording channel: a 3-file job finishing within 50 ms delivers a final progress of 3 files / 6,000 bytes before `JobDone`; a pending update precedes `JobConflict` (file-operations "The final progress is always delivered", "Progress is current when a conflict is asked")
- [x] 4.3 Confirm the core `run_job` tests (unthrottled recording sink) and the cancellation tests are unchanged

## 5. Verify

- [x] 5.1 `cargo test --workspace` green
- [x] 5.2 `cargo clippy --workspace --all-targets` with no new warnings
- [x] 5.3 `openspec validate idle-aware-event-loop --strict` clean
- [ ] 5.4 Manual (Windows): idle FileCommand shows ~0% CPU in Task Manager and the clock still ticks over; F9 after idling unfolds with animation; a pasted long path into the command line appears without per-character lag; a multi-GB copy's progress dialog updates smoothly
- [ ] 5.5 Manual (Linux, best-effort): a terminal paste into the command line arrives in one piece and is not executed
