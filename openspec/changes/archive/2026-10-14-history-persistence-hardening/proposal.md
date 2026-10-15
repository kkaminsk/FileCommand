# Change: history-persistence-hardening

## Why

`history.json` holds the command history and the Ctrl+J directory-frecency index. It has three problems from the 2026-10-12 code audit:

- **Unbounded growth, written on the UI thread on every navigation (P-08, Medium).**
  - `quicksearch::record_visit` appends every distinct directory forever.
  - `update::begin_listing` clones both histories into `Effect::PersistHistory` on every navigation.
  - `run_effects` writes the whole file synchronously on the event-loop thread.
  - A `cd` emits the effect twice, once in `dispatch_cd` and once in `begin_listing`.

  After months of use, every directory change serializes and rewrites thousands of entries on the input thread, with antivirus scanning each temp file on Windows, and the latency grows without anyone noticing.
- **Frecency recency is meaningless across restarts (R-13, Medium).** `last_visited_ms` is stamped from `state.clock_ms`, which is `RealClock::now_ms()`: milliseconds since *this process* started. The next session's clock restarts near zero, so an entry written late in a long previous session looks younger than one visited a minute ago. Ctrl+J ranking across restarts is effectively random with respect to recency. That breaks fuzzy-jump "Recency raises rank" and "History survives restart … using their persisted frecency ranking".
- **Not crash-durable and not multi-instance safe (R-15, history part, Medium).** `save_history_file_atomic` writes a temp file and renames it, but never flushes the temp file. A power loss on NTFS can leave a zero-length `history.json`. Each instance also rewrites the file from its own in-memory copy, so with two FileCommand windows open the last writer silently discards the other window's history. Both instances write the same `history.json.tmp` name too, so concurrent writes can interleave.

## What Changes

- **Wall-clock recency.**
  - Frecency timestamps are Unix-epoch milliseconds, supplied by the TUI alongside the existing monotonic tick. Animations and every other timing keep using the monotonic clock.
  - `history.json` gains a format marker.
  - A file without the marker (written by earlier versions) is migrated at load: every directory's last-visit time becomes the load time, so legacy entries rank by visit count and then age normally.
  - A last-visit time in the future, for example after a system clock correction, is clamped to the load time.
- **Bounded history.** The directory index is capped at 1,000 entries. When a new directory would exceed the cap, the lowest-frecency entry is dropped. An oversized file is pruned at load.
- **Writes leave the UI thread and are coalesced.**
  - Navigation hands the latest history snapshot to a background history writer.
  - The writer saves once a burst of changes has settled, and no later than 10 seconds after the first unwritten change.
  - Pending changes are flushed when FileCommand quits.
  - The duplicate persist in `dispatch_cd` is removed.
- **Multi-instance merge.**
  - Before each write the writer re-reads `history.json` and merges it with its snapshot. Directories are the union by path, keeping the larger visit count and the later visit time.
  - Commands that exist only on disk are kept as older than this instance's commands, and the 200-command cap still applies.
  - Two instances no longer erase each other's history.
  - The in-memory session state does not change, so Up-arrow recall and the Ctrl+J list keep showing what this instance recorded.
- **Durable writes.** The temp file has a unique name and is flushed to stable storage (`sync_all`) before it replaces `history.json`. This uses the shared durable-replace helper.
- The scope covers the whole of `history.json`, because command history shares the file and the write path. Command-line recall behavior itself does not change.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `fuzzy-jump`:
  - "Frecency ranking": recency is measured from persisted wall-clock time, with legacy-file migration and future-timestamp clamping.
  - "Directory history persistence": 1,000-entry cap pruned by lowest frecency; coalesced background writes with a final flush at quit; merge with the on-disk file before each write, covering command history too; flushed temp file before the replace.

## Impact

- **Specs:** `fuzzy-jump` (two MODIFIED requirements).
- **Code (core):**
  - `crates/filecommand-core/src/quicksearch.rs`:
    - `record_visit` enforces the cap through a new `prune_to_cap`;
    - `frecency_score`/`rank_directories` receive epoch time;
    - a new `MAX_DIR_HISTORY` constant.
  - `crates/filecommand-core/src/config.rs`:
    - `render_history_file` writes the format marker;
    - `parse_history_file`/`load_history_file` report whether the file was legacy;
    - new pure `merge_history_files` and `migrate_loaded_history`;
    - `save_history_file_atomic` uses the durable-replace helper.
  - `crates/filecommand-core/src/clock.rs`: `WallClock::epoch_ms`; `TestWallClock` gets a pinnable epoch.
  - `crates/filecommand-core/src/update.rs`:
    - `Command::Tick` also carries `epoch_ms`;
    - new `State::epoch_ms` field;
    - `begin_listing` stamps visits with `epoch_ms`;
    - fuzzy-jump ranking calls use `epoch_ms`;
    - `dispatch_cd` drops its duplicate `Effect::PersistHistory`.
- **Code (TUI):**
  - `crates/filecommand-tui/src/clock.rs`: `RealWallClock::epoch_ms` from `SystemTime`.
  - `crates/filecommand-tui/src/views/mod.rs`: the fuzzy-jump overlay passes `state.epoch_ms` instead of `state.clock_ms` to `views::fuzzy_jump::render_fuzzy_jump`, which ranks with `rank_directories` itself. Otherwise the drawn order and the reducer's Enter/cursor order would disagree.
  - `crates/filecommand-tui/src/app.rs`:
    - the main loop's `Command::Tick` carries `epoch_ms`;
    - startup history load runs the migration;
    - `Effect::PersistHistory` hands the snapshot to a new history writer instead of writing inline;
    - the writer is flushed and joined on every exit path from `run`.
  - New `crates/filecommand-tui/src/history_writer.rs`: a background thread with a single-slot latest-snapshot mailbox, debounce, and merge-then-write.
- **Tests:**
  - core unit tests for cap, prune, merge, migration, clamping, and epoch-based ranking;
  - the `update/tests.rs` `Tick` call sites move to the new shape;
  - writer tests in a temp dir for coalescing, final flush, and two-writer merge.
- **Compatibility:**
  - Old `history.json` files load and are migrated.
  - An older FileCommand reading a new file ignores the marker. It sees epoch timestamps as "very recent", which is harmless, and writes the file back without the marker, which the new version migrates again.

**Audit findings:** P-08, R-13, R-15 (history.json part)

**Coordination:**
- `config-parsing-robustness` (R-15 config part, R-16, B-05) owns `history.json` **BOM stripping** and the **JSON string-escape fix** (surrogate pairs, and no truncation on a lone surrogate) in `parse_history`/`extract_string_field`, plus the shared line tokenizer. This change does not touch those decoders. Both changes edit `config.rs`'s history section, so whichever lands second rebases onto the other's helpers.
- `editor-encoding-safety` introduces the shared durable-replace helper (temp file, `sync_all`, replace). This change routes `save_history_file_atomic` through it. If this change lands first, it introduces the helper.
- `offload-blocking-io` (R-09/P-09) moves the remaining synchronous effects off the UI thread. Only `Effect::PersistHistory` is owned here (history writer thread). `PersistTheme`/`PersistPanelSplit` are not: the audit lists those writes under P-09, so they belong to `offload-blocking-io`, which may reuse this writer pattern. (The two changes already agree: `offload-blocking-io`'s non-goals and tasks assign `PersistHistory` to this change and keep `PersistTheme`/`PersistPanelSplit` for its `ConfigWriter`, so no further coordination is needed.)
- `error-diagnostics` (B-06) logs swallowed persistence failures. Once it lands, the history writer thread routes its write failures through the same log call.
- `line-editing-in-text-fields` and `pulldown-menu-completion` (in flight) touch the fuzzy-jump dialog and the Alt+F8 command-history dialog. Neither changes how history is recorded or stored; the Alt+F8 dialog reads the in-memory command list, which this change leaves as it is.
- `idle-aware-event-loop` (P-05) changes when `Command::Tick` is delivered. Visit timestamps come from the latest tick, the same as animation start times, so that change must deliver a fresh tick (both clocks) before it applies input that follows an idle period.
- `process-lifecycle-hardening` (R-10/R-11) adds a console Ctrl+C handler and changes the quit path. Its graceful-exit paths should run the history writer's final flush.
