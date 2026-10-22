## Why

The event loop spends work where nothing changes and spends too much where a lot changes at once (audit 2026-10-12):

- **P-05 (Medium):** every 33 ms poll timeout applies `Command::Tick` and sets `dirty = true` (`app.rs:433-440`), so the full frame — `views::render`, `build_hitmap`, and the buffer diff — runs about 30 times per second forever, even with nothing on screen moving. The header clock only changes once a minute and animations last 50–760 ms. On large directories this compounds the per-frame cost, and laptop batteries pay for it.
- **P-06 (Medium):** each loop iteration reads exactly one terminal event and draws after every key. Mouse events are batched, keys are not, and bracketed paste is never enabled — so holding PgDn on a large directory, or pasting a long path, renders one frame per key and the screen lags behind the keyboard.
- **P-11 (Low):** a copy reports progress on every 256 KiB chunk with no throttling — about 10,000 `JobProgress` messages per second on NVMe, each a full reducer pass and a clone of the current file name — work that also feeds the unbounded drain loop.

## What Changes

- The loop redraws only when something changed: input or a worker result changed state, or a time-driven change became due — a frame of a running animation, a clipboard-feedback expiry, the end of the splash hold, or a minute boundary of the header clock. While idle it sleeps until the next such deadline, waking at the usual ~33 ms cadence only while worker threads that can post results are alive.
- Before applying input after an idle period, the loop brings the reducer clock up to date, so surfaces opened after idling still animate from their opening moment (the in-flight `ui-animations` model) and double-click detection compares real times.
- After handling one key, the loop keeps applying further already-queued input events — each to the state the previous one produced — up to 64 events or about 8 ms, then draws once.
- Bracketed paste is enabled on platforms whose terminal input reports pastes (crossterm 0.28 reports them only on non-Windows; on Windows a paste arrives as key events and is covered by the burst handling). A reported paste is inserted into the focused text field as one paste operation, through the same path as Ctrl+V; it is disabled on suspend, on restore, and on every exit path, and re-enabled on resume.
- Job progress reaching the UI is rate-limited to about 20 updates per second, and the latest progress is always delivered before a conflict or error question and before completion.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `application-shell`: ADDED "Idle event loop does not redraw" and "Bursts of key input render once".
- `text-field-editing`: ADDED "Terminal paste arrives as one paste". This capability is created by the in-flight `line-editing-in-text-fields` change; it is not yet in `openspec/specs/`.
- `file-operations`: ADDED "Progress updates are rate-limited".

## Impact

- **Core (`crates/filecommand-core`):** `anim.rs` — a pure "next wake" query over `State` (running animations, splash hold end, clipboard-feedback expiry); `fs_ops/job.rs` — a pure `ProgressThrottle` (offer/flush with an injected `Instant`).
- **TUI (`crates/filecommand-tui`):**
  - `app.rs` — `run`'s loop: deadline-based poll timeout, a persistent "needs draw" flag, clock refresh before input, key-burst draining (`POLL_INTERVAL` stays as the worker-alive cadence), `Event::Paste` handling; `Runtime` gains a live-worker gauge.
  - `worker.rs` — every `spawn_*` holds a gauge token for the thread's lifetime; `ChannelSink` throttles `progress` and flushes before `conflict`, `error`, and `done`.
  - `input/mod.rs` — a paste router that targets exactly the field Ctrl+V would paste into, plus editor/prompt handling.
  - `terminal.rs` — `enter_commands`, `suspend_with`/`resume_with`, and `restore_terminal` enable/disable bracketed paste (non-Windows), keeping the closure-injected order testable.
  - `clock.rs` — milliseconds until the next wall-clock minute.
- **No persisted-format, config, or dependency changes** (crossterm's `bracketed-paste` feature is on by default).

**Audit findings:** P-05, P-06, P-11

**Coordination:** `streaming-listing-performance` adds the budgeted worker-message drain and its backlog signal to this same loop; the backlog must force a zero poll timeout here — land it first, or fold its budget in when rebasing. `durable-file-copy` moves copying to a `CopyFileExW` progress routine; the throttle sits at the job sink, so progress from that routine is rate-limited without further work, and that change must keep reporting progress through `JobSink::progress`. `line-editing-in-text-fields` (in-flight) creates the `text-field-editing` capability and the `TextEditOp::Paste` path this change reuses — it must be archived before this change is archived. `ui-animations` (in-flight) defines the reducer-clock animation model the deadlines are derived from; nothing in it changes, and the pre-input `Tick` (design D4) keeps open stamps fresh after idling. `history-persistence-hardening` turns `Tick` into `{ now_ms, epoch_ms }` and relies on that same pre-input tick for fresh frecency timestamps; whichever lands second makes the pre-input tick carry both clocks. `resize-preserves-session-state` and `process-lifecycle-hardening` touch the same loop (resize handling, quit/Ctrl+C) and should rebase onto whichever lands first.
