# Design: history-persistence-hardening

## Context

This was verified against `build/open-proposals-2026-10` at `1bada6f`.

- **One file, two histories.** `config::HistoryFile { commands: Vec<String>, directories: Vec<FrecencyEntry> }` is the whole of `history.json`.
  - `render_history_file` writes `{"commands": [...], "directories": [{"path", "visit_count", "last_visited_ms"}, ...]}`.
  - `parse_history_file` extracts the two arrays with `extract_value_span`.
  - `load_history_file` is called once in `app::run`, which copies the result into `state.history` and `state.dir_history`.
  - Command history is ordered oldest to newest and capped at `MAX_HISTORY = 200` by `push_history`. Today it only grows from a successful `cd` in `update::dispatch_cd`.
- **Recording.** `update::begin_listing` is the navigation choke point. It calls:
  - `quicksearch::record_visit(&mut state.dir_history, &path, state.clock_ms)`, which does a linear `find` and then either increments the count and refreshes the time, or appends a new entry with no cap;
  - `begin_listing_inner`;
  - then it pushes `Effect::PersistHistory(HistoryFile { commands: state.history.clone(), directories: state.dir_history.clone() })`.

  `dispatch_cd` pushes the command, emits its own `Effect::PersistHistory`, and then calls `begin_listing`, which emits a second one. The stale-tab refresh and the Tree preview bypass this through `begin_listing_inner`. Ctrl+R and job-completion re-reads do go through `begin_listing`, so they also count as visits and persists. That is existing behavior, unchanged here.
- **Writing.** `run_effects` handles `Effect::PersistHistory(file)` on the event-loop thread with `config::save_history_file_atomic(&rt.history_path, &file)`, skipped when `rt.persist_enabled` is false:
  - it calls `std::fs::write` on the fixed sibling `history.json.tmp`, then `std::fs::rename`;
  - there is no `sync_all`;
  - there is no read of the current file, so with two instances the last writer wins.
- **Clock.** `filecommand-tui/src/clock.rs`:
  - `RealClock::now_ms` returns `Instant::elapsed()` since `RealClock::new()` at process start.
  - The main loop sends `Command::Tick(clock.now_ms())` whenever `event::poll(POLL_INTERVAL)` times out, and `update` stores it in `state.clock_ms` (`update.rs`, top of `update_impl`).
  - `state.clock_ms` drives animations (`dialog_opened_at_ms`, the splash hold), double-click detection, and clipboard-feedback expiry, and also frecency: `record_visit`, and `rank_directories(…, state.clock_ms)` both in the Ctrl+J reducer handlers and in the TUI's `render_fuzzy_jump` (called from `views/mod.rs`).
  - `frecency_score` is `visit_count * (RECENCY_WINDOW - min(age, RECENCY_WINDOW) + 1)`, with a one-week linear window (the constant is named `RECENCY_HALF_LIFE_MS`, but the decay is linear). Across sessions, `now.saturating_sub(last)` compares two unrelated time bases.
  - `WallClock` exists in core for the on-screen `h:mm` clock and returns only `(hour, minute)`.

## Goals / Non-Goals

**Goals:**

- Ctrl+J recency means the same thing across restarts, and existing `history.json` files migrate without losing visit counts.
- The directory index stays bounded, and the size of the per-navigation work does not depend on how long the user has used FileCommand.
- No `history.json` write happens on the UI thread, and bursts of navigation coalesce into one write.
- Two concurrent instances do not erase each other's commands or directories.
- A power loss during a write leaves either the old or the new complete file.

**Non-Goals:**

- BOM tolerance and JSON escape decoding (surrogate pairs, and no truncation on a lone `\uD83D`) in the history parser. These belong to `config-parsing-robustness` (B-05, R-15 config part), which owns the shared tokenizer and the decoders.
- `config.toml` durability (`config-parsing-robustness`) and moving the theme and split writes off the UI thread (`offload-blocking-io`, because the audit lists those persist writes under P-09).
- Changing which navigations count as visits, the frecency formula, or the one-week window.
- Live sharing between running instances. Each instance sees the other's entries after its own next restart.
- A cross-process lock that would close the millisecond-scale race described in Risks.

## Decisions

### D1: Epoch milliseconds arrive with the existing tick; the monotonic clock stays for animations

- **The tick.** `Command::Tick(u64)` becomes `Command::Tick { now_ms: u64, epoch_ms: u64 }`. `update` stores `now_ms` in `state.clock_ms` exactly as today, and stores `epoch_ms` in a new field, `state.epoch_ms`.
- **Who reads which clock.** `record_visit` and every `rank_directories`/`frecency_score` call read `state.epoch_ms`. That includes the TUI renderer: `views::mod` passes `state.clock_ms` to `views::fuzzy_jump::render_fuzzy_jump` today, which calls `rank_directories` to draw the list, so it must pass `state.epoch_ms` too, or the drawn order would differ from the order the reducer's cursor and Enter act on. Nothing else reads the epoch: animations, the splash hold, and clipboard-feedback expiry keep using `state.clock_ms`, so a wall-clock correction during a session cannot stall or skip an animation.
- **Where epoch time comes from.** `WallClock` gains `fn epoch_ms(&self) -> u64`. `RealWallClock` implements it with `SystemTime::now().duration_since(UNIX_EPOCH)`, and `TestWallClock` gets a pinnable epoch.
- **Startup.** `State::initial` takes the initial epoch, so visits recorded before the first tick are stamped correctly.
- **The reducer still reads no clock.** Both values come in through the command, which keeps "deterministic under a pinned clock".

Alternatives considered:

- **A one-time epoch offset (`epoch = clock_ms + offset_at_start`).** This avoids changing `Tick`, but `Instant` does not advance during system sleep on every platform, so the derived epoch drifts behind by the total suspended time of a long-running session.
- **A separate `Command::WallTime` message.** This doubles the commands per idle poll for no benefit.
- **Letting the TUI write `state.epoch_ms` directly before each `apply`.** This breaks the rule that `update`'s inputs arrive only as commands.

### D2: Format marker plus load-time migration

- `render_history_file` writes a top-level `"version": 2` key.
- `load_history_file` reports whether the key was absent, which marks a legacy file whose timestamps are session-relative. The key is looked up the same way as `"commands"` and `"directories"`; if `config-parsing-robustness` has landed, that is its top-level-only key lookup, so a `"version"` inside a command string is never mistaken for the marker.
- A new pure function, `config::migrate_loaded_history(file, legacy, now_epoch)`, runs at startup:
  - for a legacy file, every directory's `last_visited_ms` is set to `now_epoch`;
  - for any file, a `last_visited_ms` greater than `now_epoch` is clamped to `now_epoch`;
  - the directory list is pruned to the cap (D3).

Setting legacy entries to "now" follows the audit's recommendation. Right after an upgrade the user's established directories rank by visit count, ahead of anything new, and then decay normally over the one-week window as fresh visits take over.

Alternatives considered:

- **Setting legacy timestamps to 0 ("ancient").** Every directory visited after the upgrade would immediately outrank a directory with hundreds of past visits, which is a worse first week.
- **Guessing legacy files by value magnitude** (timestamps below roughly 1e12 are monotonic). This works in practice, but it is heuristic, and the marker is explicit and also future-proofs the format.

The clamp protects against a clock that was ahead and then corrected: without it, an entry stamped in the future stays at maximum recency until real time catches up.

### D3: Cap the directory index at 1,000, pruning the lowest frecency

`quicksearch::MAX_DIR_HISTORY = 1_000`. `record_visit(history, path, now)` behaves as today, then calls `prune_to_cap(history, MAX_DIR_HISTORY, now)` when the list exceeds the cap:

- it removes the entry with the lowest `frecency_score` at `now`;
- among equal scores it removes the one that appears last in list order, so the result is deterministic;
- it never removes the entry just visited. That entry is excluded explicitly rather than relying on its score: a one-visit entry stamped now can tie with another one-visit entry stamped in the same millisecond, and the tie rule above would then pick the newest entry, which is the visited one.

The same prune runs after migration at load and after every merge (D5). Each visit is O(n) for n ≤ 1,000. This keeps the per-navigation `find` and the snapshot clone bounded.

Alternatives considered:

- **Time-based expiry (drop entries older than N days).** This can drop a heavily used directory after a vacation.
- **Least recently visited.** This ignores frequency, which frecency exists to capture.

### D4: A background history writer with a latest-snapshot mailbox, debounce, and a final flush

The reducer keeps emitting `Effect::PersistHistory(HistoryFile)` from `begin_listing`. `dispatch_cd`'s duplicate emission is deleted: `begin_listing` already runs after `push_history` and snapshots both lists. The snapshot clone is bounded by D3 (≤1,000 directories, ≤200 commands).

In the TUI, `run_effects` no longer writes. It calls `rt.history_writer.submit(file)`, which posts the snapshot to a new `history_writer` thread spawned in `app::run` only when `persist_enabled` is true; otherwise `submit` is a no-op. The thread:

- loops on `recv_timeout`, keeping only the newest snapshot it has received (a single-slot mailbox: older snapshots are dropped unread);
- writes when 2 s have passed with no new snapshot, or when 10 s have passed since the oldest unwritten snapshot arrived, whichever comes first;
- on a `Flush` message, writes any pending snapshot and exits.

`HistoryWriter` implements `Drop`: it sends `Flush` and joins the thread with a bounded wait (2 s), so that a hung network profile directory cannot hold the process open. `Runtime` owns the writer, so every exit from `run` flushes: the `return Ok(())` quit sites, early `?` error returns, and panic unwinding.

Alternatives considered:

- **Debouncing in the reducer**, with a dirty flag flushed on a later `Tick`. This depends on ticks continuing to arrive, and `idle-aware-event-loop` intends to stop idle ticks. It also still writes on the UI thread.
- **A spawned thread per write.** Concurrent writers would race each other for the same file.
- **Writing only at quit.** A crash would lose the whole session's history.

### D5: Merge with the on-disk file before every write

Before each write the writer calls `config::load_history_file(path)`, migrates the disk copy's timestamps if it is legacy, and then calls the pure `config::merge_history_files(disk, mine, now_epoch) -> HistoryFile`:

- **Directories.** The result is the union by path, using the same `PathBuf` equality `record_visit` uses.
  - For a path present in both, `visit_count = max` and `last_visited_ms = max`.
  - Order: this instance's entries first, in their order, then disk-only entries in disk order. This preserves `rank_directories`' tie order for the instance's own entries.
  - The union is then pruned to `MAX_DIR_HISTORY` (D3).
- **Commands.** Disk-only commands (those not present in `mine.commands`) come first, in disk order, followed by `mine.commands`. The oldest entries are dropped to fit `MAX_HISTORY`. Commands that only the other instance ran therefore rank as older than this instance's own, and nothing is duplicated.

`max` is idempotent: merging the same pair twice gives the same file, so repeated flushes from either instance never inflate counts. The cost is that concurrent visits to the same directory from two instances count as the larger of the two increments, not their sum.

The merged result is written to disk only. The writer does not send it back to the reducer, so this instance's Up-arrow recall and Ctrl+J list keep showing what it recorded itself. That leaves command-line "Command history navigation" (an in-flight requirement) untouched: its "Executed command persisted to history" scenario still holds, because the command is appended to the in-memory history at once and `history.json` is still replaced atomically, now within seconds instead of inline. The Alt+F8 command-history dialog (pulldown-menu-completion, in flight) also lists the in-memory history, so it is unaffected too. Because every flush re-merges the instance's whole in-memory state, the other instance's entries survive every later write.

Alternatives considered:

- **Replaying only this instance's new commands and visits on top of the disk file** (a journal of deltas). This gives exact counts, but it needs per-flush bookkeeping and is not idempotent if a write fails halfway.
- **Feeding the merged result back into `State`.** This changes in-session recall under the user and needs a new command, for little benefit.

### D6: Durable write through the shared helper

`save_history_file_atomic` becomes `durable::replace_file(path, render_history_file(&merged).as_bytes())`:

- the temp file has a unique name, `.<name>.<pid>.<n>.fc-tmp`, in the same directory, so two instances never share a temp file;
- the temp file is flushed with `sync_all` before the replace;
- on failure the temp file is removed and `history.json` is left as it was.

The helper comes from `editor-encoding-safety` (D4 there). Failures stay silent, as today: a history write must never take the session down. Reporting them is `error-diagnostics`' concern.

## Risks / Trade-offs

- **[Risk] Lost update between two instances that flush within the same few milliseconds.** Both read the disk copy, then both write, and the second write lacks the first one's newest entries. → Each instance re-merges its whole in-memory state on every later flush, so only an instance's final flush at exit can be overwritten for good. A lock file would close this gap, at the cost of stale-lock handling. It is not done here.
- **[Risk] Up to 10 s of history lost on a hard crash or a power loss.** → This is accepted in exchange for taking writes off the input path. Graceful exits flush. `process-lifecycle-hardening` should route Ctrl+C through the same exit path.
- **[Risk] Running an old and a new FileCommand at the same time.** The old one writes legacy, session-relative timestamps. Merged by the new writer, those values lose every `max` comparison, and disk-only legacy entries look decades old (they rank by visit count only). → This is a rare, transitional mix. Nothing is lost, and only the recency of those entries is understated.
- **[Trade-off] Max-merge undercounts concurrent visits to the same directory.** → This is acceptable for a ranking heuristic, and it is the idempotent choice.
- **[Risk] A hung profile directory (an offline network path) stalls the writer thread.** → Only the writer stalls. The UI never waits, and the final-flush join is bounded at 2 s.

## Migration Plan

- **First launch after upgrade:** the file has no `"version"` key. Every directory's last-visit time is set to the load time and the index is pruned to 1,000. The first write adds `"version": 2`. Visit counts and command history carry over unchanged.
- **Downgrade:** older builds ignore the `"version"` key. They see the epoch timestamps as "just visited" (saturating age 0) and write the file back without the key. The next upgraded launch migrates it again. Nothing is lost.

## Open Questions

None.
