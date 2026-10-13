# Tasks: listing-request-guard

## 1. Core: listing request ids (`crates/filecommand-core`)

- [ ] 1.1 `update.rs`: add `request: u64` to `Effect::StartListing`, `Command::ListingChunk`, `Command::ListingComplete`, `Command::ListingFailed`; add `Effect::CancelRequest(u64)` with a doc comment naming the three cancellable request kinds (design D1, D3)
- [ ] 1.2 `panel.rs`: add `PanelState::listing_request: Option<u64>` and `TabData::listing_request`, carried by `to_tab_data`/`adopt_tab_data` beside `git_request` (design D1)
- [ ] 1.3 `update.rs`: mint the id with `State::next_request_id` in `State::initial` (both panels) and in `begin_listing_inner`, storing it in `listing_request` after `begin_new_listing` (design D1)
- [ ] 1.4 `update.rs` `begin_listing_inner`: when the panel's previous listing is still `Streaming`, push `Effect::CancelRequest(previous)` before `StartListing` (panel-navigation "Superseded directory listings are discarded and cancelled"; design D4)
- [ ] 1.5 `update.rs` `apply_listing_event`: return early, before any mutation (entries, progress, reread anchor, `pending_cursor_target`, selection reconcile), when `request != listing_request` (design D2)
- [ ] 1.6 `update.rs` `update()`: clear `state.drag` on `ListingFailed` only when the failure was accepted by 1.5 (design D2)
- [ ] 1.7 Update `handle_swap_panels`' doc comment to say the streaming refusal is kept by spec, not because chunks lack an id; behavior unchanged (design D7)

## 2. Core: tabs

- [ ] 2.1 `panel.rs` `to_tab_data`: set `stale: self.progress.is_streaming()` and update its doc comment (panel-tabs "Switching or closing a tab mid-listing never mixes listings"; design D6)
- [ ] 2.2 `update.rs` `Command::SwitchTab`: before switching, if the active tab is `Streaming`, push `Effect::CancelRequest` for its `listing_request` (design D4)
- [ ] 2.3 `update.rs` `Command::CloseTab`: if the tab being closed is `Streaming`, push `Effect::CancelRequest` for its `listing_request` (design D4)
- [ ] 2.4 Confirm `Command::OpenTab` emits no cancel and the new tab inherits `listing_request` (design D4)
- [ ] 2.5 `update.rs` stale-tab activation path (`SwitchTab`/`CloseTab`): call `begin_listing_inner(.., capture_anchor: false)` when the newly active tab's progress is `Streaming`, so an interrupted re-read's stashed `reread_anchor` survives; keep `true` otherwise (panel-tabs "Switching or closing a tab mid-listing never mixes listings", "Stale background tab refresh on activation"; design D6)

## 3. Core: find-file and viewer cancellation

- [ ] 3.1 `listing.rs`: `find_in_subtree` takes `cancel: &CancelFlag`, checks before each directory read and between entries, and returns early when set (find-file "Dismissed or superseded searches stop"; design D5)
- [ ] 3.2 `listing.rs`: `list_dir_chunked` takes `cancel: &CancelFlag` and stops delivering chunks once set (design D5)
- [ ] 3.3 `viewer/search.rs`: `find_forward` takes `cancel: &CancelFlag`, checks before each chunk read, returns `None` once set (viewer "Search stops when superseded or when the viewer closes"; design D5)
- [ ] 3.4 `update.rs` `handle_find_file`: `FindFileCancel` and `FindFileConfirm` push `Effect::CancelRequest(request)` when a search was submitted and is not `done` (design D4)
- [ ] 3.5 `update.rs` `handle_viewer`: `ViewerClose` pushes `Effect::CancelRequest` for `search_request` when set; `ViewerSearchConfirm` pushes it for the previous `search_request` before minting the new one (design D4)

## 4. TUI: workers and cancel registry (`crates/filecommand-tui`)

- [ ] 4.1 `app.rs`: add a `CancelRegistry` (`register(id) -> CancelFlag`, `cancel(id)`, `retire(id)`) to `Runtime` (design D3)
- [ ] 4.2 `app.rs` `run_effects`: register a flag for `StartListing`, `FindInSubtree`, and `RunViewerSearch` and pass it to the worker; execute `CancelRequest(id)` via `cancel` (unknown ids are a no-op) (design D3)
- [ ] 4.3 `app.rs` `apply`: `retire` the entry when `ListingComplete`, `ListingFailed`, `FindFileSearchDone`, or `ViewerSearchResult` for that id passes through, before `update` consumes it (design D3)
- [ ] 4.4 `worker.rs` `spawn_listing`: take `request` and the flag; echo `request` on every reply; check the flag before the `..` chunk, before each chunk, and before the final reply; send nothing after cancellation (design D5)
- [ ] 4.5 `worker.rs` `spawn_find_subtree` and `spawn_viewer_search`: take the flag, pass it to `find_in_subtree`/`find_forward`, and send no `FindFileSearchDone`/`ViewerSearchResult` once cancelled (design D5)

## 5. Tests

- [ ] 5.1 `update/tests.rs`: add `listing_chunk`/`listing_complete`/`listing_failed` helpers that read the panel's current `listing_request`, and migrate the existing listing-command call sites (design D8)
- [ ] 5.2 `update/tests.rs`: enter `C:\big`, Backspace to `C:\` mid-stream, feed a chunk (including `..`) and a `ListingComplete` with the old id → `C:\` entries unchanged, still `Streaming`, `CancelRequest(old)` was emitted (panel-navigation "Backspace out of a slow directory mid-listing", "Superseded completion does not complete the new listing")
- [ ] 5.3 `update/tests.rs`: a stale `ListingFailed` sets no `last_error` and does not clear an active drag (panel-navigation "Superseded failure does not mark the new listing failed"; design D2)
- [ ] 5.4 `update/tests.rs`: two `RereadPanel`s, chunks from both ids fed → each entry present once; first id cancelled (panel-navigation "Two quick re-reads do not duplicate entries")
- [ ] 5.5 `update/tests.rs`: `SwitchTab` mid-stream → `CancelRequest(old)`, stashed tab `stale`, chunk with old id ignored by the new tab; switching back emits a fresh `StartListing` and no `PersistHistory` (panel-tabs "Switching tabs mid-listing", "Returning to an interrupted tab re-reads it")
- [ ] 5.5a `update/tests.rs`: on a complete listing with the cursor on `report.txt` and a non-zero offset, `RereadPanel`, then `SwitchTab` away mid-stream and back → the fresh listing's chunks put the cursor back on `report.txt` at the old offset; a tab interrupted mid-*navigation* refreshes from the top; switching to a non-stale, fully listed tab emits no `StartListing` (panel-tabs "An interrupted re-read still keeps the user's place", "Switching to a tab with no pending staleness is unchanged")
- [ ] 5.6 `update/tests.rs`: `CloseTab` mid-stream → `CancelRequest(old)`, neighbor untouched by old-id chunks (panel-tabs "Closing a tab mid-listing")
- [ ] 5.7 `update/tests.rs`: `OpenTab` mid-stream → no `CancelRequest`, later chunks fill the new tab, original tab stale (panel-tabs "New tab mid-listing keeps the listing")
- [ ] 5.8 `update/tests.rs`: `FindFileCancel` and `FindFileConfirm` while running emit `CancelRequest(request)`; after `FindFileSearchDone` they emit none (find-file "Esc stops the walk", "Confirming a result while the walk is running stops it")
- [ ] 5.9 `update/tests.rs`: `ViewerClose` with an issued search and a second `ViewerSearchConfirm` emit `CancelRequest` for the earlier id (viewer "Closing the viewer stops a running search", "A newer search supersedes a running one")
- [ ] 5.10 `update/tests.rs`: Ctrl+U is still refused while a panel streams (regression for the unchanged in-flight rule)
- [ ] 5.11 `listing.rs` tests: `find_in_subtree` with a counting fake reader stops reading directories after the flag is set from inside `on_match`; `list_dir_chunked` with a pre-set flag delivers nothing (find-file "Esc stops the walk"; panel-navigation "Superseded reader stops")
- [ ] 5.12 `viewer/search.rs` tests: `find_forward` with a pre-set flag returns `None` without scanning past the first chunk check
- [ ] 5.13 `worker.rs` tests: `spawn_listing` and `spawn_find_subtree` given a pre-cancelled flag send nothing on the channel within the timeout; a normal run echoes the request id on every reply
- [ ] 5.14 `app.rs` tests: `CancelRegistry` — `cancel` sets the registered flag and removes it, `cancel` of an unknown id is a no-op, `retire` removes without setting
- [ ] 5.15 Update `tests/integration_smoke.rs` and `tests/snapshot_views.rs` listing-command call sites; snapshots unchanged

## 6. Verify

- [ ] 6.1 `cargo test --workspace` green
- [ ] 6.2 `cargo clippy --workspace --all-targets` with no new warnings
- [ ] 6.3 `openspec validate listing-request-guard --strict` clean
- [ ] 6.4 Manual (Windows): enter a large directory on a network share and press Backspace immediately → parent shows only its own entries; switch tabs mid-listing and back → the tab re-reads; Alt+F7 from `C:\` with pattern `a`, Esc → disk activity stops
