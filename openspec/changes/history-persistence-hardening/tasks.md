# Tasks: history-persistence-hardening

## 1. Wall-clock time in the reducer (R-13)

- [ ] 1.1 `crates/filecommand-core/src/clock.rs`: add `WallClock::epoch_ms(&self) -> u64` and give `TestWallClock` a pinnable epoch (constructor or setter) (fuzzy-jump "Frecency ranking"; design D1).
- [ ] 1.2 `crates/filecommand-tui/src/clock.rs`: implement `RealWallClock::epoch_ms` with `SystemTime::now().duration_since(UNIX_EPOCH)`, saturating to 0 if the clock reads before the epoch.
- [ ] 1.3 `crates/filecommand-core/src/update.rs`: change `Command::Tick(u64)` to `Command::Tick { now_ms, epoch_ms }`, add `State::epoch_ms`, set both fields in the top-of-`update` tick handling, and update the splash and other `Tick` match arms. `State::initial` takes the initial epoch (design D1).
- [ ] 1.4 Stamp visits in `begin_listing` with `state.epoch_ms`, and pass `state.epoch_ms` to every `quicksearch::rank_directories` call in the Ctrl+J handlers and to `render_fuzzy_jump` in `crates/filecommand-tui/src/views/mod.rs`, so the drawn list and the reducer rank with the same time. Leave every animation, splash, double-click, and clipboard-feedback use of `state.clock_ms` unchanged (design D1). Add a test that the reducer's Enter target equals the first row the renderer draws when `clock_ms` and `epoch_ms` differ widely.
- [ ] 1.5 `crates/filecommand-tui/src/app.rs`: send `Command::Tick { now_ms: clock.now_ms(), epoch_ms: wall_clock.epoch_ms() }` from the main loop, and pass the epoch to `State::initial`. Update `crates/filecommand-tui/tests/integration_smoke.rs` and the `update/tests.rs` `Tick` call sites to the new shape.

## 2. Format marker, migration, and cap (R-13, P-08)

- [ ] 2.1 `crates/filecommand-core/src/config.rs`: `render_history_file` writes `"version": 2`. `load_history_file`/`parse_history_file` report whether the key was absent (legacy) without changing the tolerance for malformed files (design D2).
- [ ] 2.2 Add the pure function `config::migrate_loaded_history(&mut HistoryFile, legacy, now_epoch)`. For a legacy file it sets every `last_visited_ms` to `now_epoch`; for any file it clamps future values to `now_epoch`; then it prunes to the cap (fuzzy-jump "Legacy history file is migrated at load", "Future last-visit time is clamped"; design D2).
- [ ] 2.3 `crates/filecommand-core/src/quicksearch.rs`: add `MAX_DIR_HISTORY = 1_000` and `prune_to_cap(history, cap, now)`. `prune_to_cap` removes the lowest `frecency_score` first, and among equal scores the entry that comes last in list order. Make `record_visit` call it after an insert that exceeds the cap, never pruning the visited path (fuzzy-jump "History is capped by frecency"; design D3).
- [ ] 2.4 `app::run`: apply `migrate_loaded_history` to the loaded file with the startup epoch before copying it into `State`.

## 3. Merge and durable write (R-15 history part)

- [ ] 3.1 Add the pure function `config::merge_history_files(disk, mine, now_epoch) -> HistoryFile`:
  - directories: union by path with `max` count and `max` time, this instance's entries first, then pruned to `MAX_DIR_HISTORY`;
  - commands: disk-only commands first, in disk order, then `mine.commands`, keeping the newest `MAX_HISTORY` (fuzzy-jump "Two instances keep each other's history", "Merge keeps the larger count and the later visit", "Another instance's commands are kept"; design D5).
- [ ] 3.2 Route `save_history_file_atomic` through the shared durable-replace helper: a unique temp name in the same directory, `sync_all` before the replace, and temp-file cleanup on failure. Introduce the helper here if `editor-encoding-safety` has not landed yet. The legacy plain-array `save_history_atomic` has no caller outside `config.rs` tests and also uses the fixed-name `temp_sibling`; route it through the helper too, or delete it with its tests, so no `history.json` write keeps the shared temp name (fuzzy-jump "Interrupted write leaves a complete file"; design D6).

## 4. Background history writer (P-08)

- [ ] 4.1 New `crates/filecommand-tui/src/history_writer.rs`: `HistoryWriter::spawn(path, debounce, max_wait)`, `submit(HistoryFile)`, and an internal `Flush`. The thread keeps only the newest snapshot. It writes after 2 s with no new snapshot or 10 s after the oldest unwritten one. Each write is load → migrate the disk copy if legacy → `merge_history_files` → durable write (fuzzy-jump "A burst of navigation is written once"; design D4).
- [ ] 4.2 `HistoryWriter` implements `Drop`: it sends `Flush` and joins with a bounded 2 s wait. Hold the writer in `Runtime`, spawned only when `persist_enabled`, so every exit path from `run` flushes (fuzzy-jump "Quitting flushes unwritten history"; design D4).
- [ ] 4.3 `run_effects` → `Effect::PersistHistory(file)`: replace the inline `save_history_file_atomic` call with `rt.history_writer.submit(file)` (fuzzy-jump "Navigation does not wait for the history write").
- [ ] 4.4 `update.rs` `dispatch_cd`: remove the duplicate `Effect::PersistHistory`. The `begin_listing` that follows already snapshots the updated command history (design D4).

## 5. Tests

- [ ] 5.1 `quicksearch.rs`:
  - `prune_to_cap` removes the lowest score and breaks ties by the last list position;
  - `record_visit` on a full 1,000-entry history adds the new path, keeps the length at 1,000, and never prunes the visited path;
  - revisiting an existing path in a full history prunes nothing.
- [ ] 5.2 `quicksearch.rs`/`update/tests.rs` R-13 regression: an earlier "session" stamps `C:\old` at epoch E and `C:\new` at E + 3 days. A fresh `State` with `epoch_ms` = E + 3 days + 1 min ranks `C:\new` first (fuzzy-jump "Recency survives a restart").
- [ ] 5.3 `config.rs`:
  - a legacy file (no `"version"`) migrates every `last_visited_ms` to now;
  - a v2 file keeps its timestamps except future ones, which clamp to now;
  - an oversized file prunes to 1,000;
  - a render/parse round trip keeps `"version": 2`;
  - the existing malformed-file tests still yield an empty history.
- [ ] 5.4 `config.rs` `merge_history_files`:
  - disjoint directories union;
  - an overlapping path takes `max` count and `max` time;
  - merging is idempotent (merging the result with `mine` again gives the same result);
  - disk-only commands come first;
  - duplicates are not repeated;
  - the 200-command cap keeps the newest;
  - a merged directory union above 1,000 is pruned.
- [ ] 5.5 `history_writer.rs` (temp dir, short debounce injected):
  - 20 rapid submits produce one write containing the last snapshot;
  - a submit followed by drop writes before `drop` returns;
  - `submit` with persistence disabled writes nothing.
- [ ] 5.6 `history_writer.rs` (temp dir): two writers on one path, each submitting a distinct directory and then dropped, leave a file containing both directories. Also, `history.json` already holding a foreign command keeps it after a write.
- [ ] 5.7 `update/tests.rs`: a successful `cd` emits exactly one `Effect::PersistHistory`, and its snapshot contains the new command and the new visit.
- [ ] 5.8 Durable write: with the helper's fault-injection seam, a failure before the replace leaves the previous `history.json` byte-identical and no `.fc-tmp` sibling.

## 6. Docs

- [ ] 6.1 README `history.json` section: replace "written atomically so a crash mid-write never corrupts it" with the actual guarantees (flushed and atomically replaced; written in the background within seconds and at quit; merged across concurrent instances; directory history capped at 1,000 entries).

## 7. Verify

- [ ] 7.1 `cargo test --workspace` green.
- [ ] 7.2 `cargo clippy --workspace --all-targets` with no new warnings.
- [ ] 7.3 `openspec validate history-persistence-hardening --strict` clean.
- [ ] 7.4 Manual check:
  - start with an existing pre-change `history.json`, and confirm Ctrl+J lists the old directories ranked by count and that the file gains `"version": 2` after a navigation;
  - run two instances, navigate in both, close both, and confirm both sets of directories are in the file;
  - navigate quickly through many directories and confirm the UI never stutters and the file updates a few seconds later.
