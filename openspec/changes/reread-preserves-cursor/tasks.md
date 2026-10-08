# Tasks: reread-preserves-cursor

## 1. Core panel state (`crates/filecommand-core/src/panel.rs`)

- [ ] 1.1 Add `RereadAnchor { names: Vec<OsString>, cursor: usize, scroll_offset: usize }` and `Panel::reread_anchor: Option<RereadAnchor>`, initialized to `None` (design D1, D4)
- [ ] 1.2 Include `reread_anchor` in the per-tab stash/restore data beside `pending_cursor_target` so a tab switched away mid-re-read keeps it (design D4)
- [ ] 1.3 Add `Panel::capture_reread_anchor()` — records entry names in display order, cursor, and scroll offset only when `progress` is `Complete`; otherwise leaves the anchor `None` (design D5)
- [ ] 1.4 Add `Panel::apply_reread_anchor(rows)` — resolves the cursor in the order `names[cursor]`, `names[cursor + 1..]`, `names[..cursor]` reversed; on a hit sets `cursor` and `cursor_user_moved = true`, restores `scroll_offset` and clamps it to `visible_len.saturating_sub(rows)`; returns whether a name resolved (panel-navigation: "Same-directory re-read preserves cursor and scroll position"; design D1, D3)
- [ ] 1.5 In `begin_new_listing(cwd)`, clear `reread_anchor` when `cwd != self.cwd` so a navigation never inherits a stale anchor (design D4)

## 2. Reducer (`crates/filecommand-core/src/update.rs`)

- [ ] 2.1 In `begin_listing_inner`, call `capture_reread_anchor()` before `begin_new_listing` when `path == panel.cwd` (design D5)
- [ ] 2.2 In `apply_listing_event`, on `ListingChunk` call `apply_reread_anchor(rows)` after the chunk is inserted and before `reconcile_panel_viewport` (panel-navigation: "No jump while the re-read streams"; design D2)
- [ ] 2.3 In `apply_listing_event`, on `ListingComplete` apply `pending_cursor_target` first (unchanged), then `apply_reread_anchor(rows)` if no find-file target was consumed, then take the anchor so it is consumed, then `reconcile_panel_viewport` (design D2)
- [ ] 2.4 Route the Brief-mode offset restore through `ensure_cursor_visible_brief` so the restored offset stays on a whole-column multiple (design D3)

## 3. Tests

- [ ] 3.1 `update/tests.rs`: `JobDone` delete of the cursored entry lands the cursor on the following entry with the offset unchanged (panel-navigation: "Deleting the cursored file lands on the next entry"; file-operations: "Post-delete re-read keeps the user's place")
- [ ] 3.2 `update/tests.rs`: deleting the last entry lands on the new last entry (panel-navigation: "Deleting the last entry lands on the previous one")
- [ ] 3.3 `update/tests.rs`: multi-selection delete `{b, c, d}` with the cursor on `c` lands on `e` (panel-navigation: "Multi-selection delete lands on the first survivor at or after the cursor")
- [ ] 3.4 `update/tests.rs`: 60 entries, 20-row body, offset 30, cursor at position 40; delete the cursored entry → offset 30, cursor at position 40 (panel-navigation: "Scrolled window stays put")
- [ ] 3.5 `update/tests.rs`: offset 40, cursor on the last of 60; delete it → 59 entries, offset 39, cursor on the last entry (panel-navigation: "Deleting at the very bottom pulls the window up by one"; "Re-read anchor restore re-clamps against the shorter list")
- [ ] 3.6 `update/tests.rs`: `RereadPanel` keeps the cursor on `report.txt` with the offset unchanged (panel-navigation: "Ctrl+R keeps the cursor's entry")
- [ ] 3.7 `update/tests.rs`: the first `ListingChunk` of a same-directory re-read containing the anchored name already places the cursor on it (panel-navigation: "No jump while the re-read streams")
- [ ] 3.8 `update/tests.rs`: Enter on a subdirectory after a re-read starts the new listing at cursor 0 / offset 0 (panel-navigation: "Navigating to a different directory still starts at the top")
- [ ] 3.9 `update/tests.rs`: extend `job_done_delete_rereads_both_panels_when_both_browse_the_same_directory` to assert the inactive panel's cursor entry and offset are also preserved (file-operations: "The opposite panel sharing the affected directory also refreshes")
- [ ] 3.10 `update/tests.rs`: a re-read issued while the previous listing is still `Streaming` captures no anchor and behaves as before (design D5)
- [ ] 3.11 `panel.rs`: unit tests for `apply_reread_anchor`'s resolution order and for the Brief-mode column-aligned offset restore (design D1, D3)
- [ ] 3.12 Confirm the existing `job_done_*`, `reread_*`, and find-file settle tests still pass unchanged

## 4. Verify

- [ ] 4.1 `cargo test --workspace` green
- [ ] 4.2 `openspec validate reread-preserves-cursor --strict` clean
- [ ] 4.3 Manual: in a directory longer than the panel, scroll partway down, F8 a file → no jump, cursor on the following entry; Ctrl+R → no jump; Enter into a subdirectory → listing starts at the top
