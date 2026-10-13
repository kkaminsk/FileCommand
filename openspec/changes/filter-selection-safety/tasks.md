# Tasks: filter-selection-safety

## 1. No current entry under an empty filter (`crates/filecommand-core/src/panel.rs`)

- [ ] 1.1 Make `PanelState::selected()` return `None` when `quick_filter` is `Some(p)` and the cursor entry fails `matches_quick_filter(entry, p)`. The check matches the single entry and does not allocate `visible_indices()` (quick-filter "Cursor and mini-status behavior under an active filter"; design D1).
- [ ] 1.2 Make `toggle_selection_and_advance` (Ins) take its entry from `selected()`, so it toggles nothing when there is no current entry (quick-filter "Ins does nothing while no entry is visible"; design D1).
- [ ] 1.3 Leave `cursor` untouched when nothing is visible, and confirm that `snap_cursor_to_visible` moves it to the nearest visible entry once the pattern widens (quick-filter "Widening the pattern restores a current entry").

## 2. Hidden-selection disclosure (R-17)

- [ ] 2.1 Add `PanelState::hidden_selected_count(&[SourceItem]) -> usize`: the number of sources that lie in the panel's `cwd`, are in its selection set, and whose entry fails the active `quick_filter`; 0 with no filter (design D2).
- [ ] 2.2 `crates/filecommand-core/src/fs_ops/dialog.rs`: add `hidden_by_filter: usize` to `FileOpSetup::DestinationInput` and `FileOpSetup::DeleteConfirm`. Set it to 0 for Mkdir.
- [ ] 2.3 `crates/filecommand-core/src/update.rs`: compute the field from the **source** panel in `enter_file_op_setup_for_sources` and `enter_delete_confirm_for_sources`. That covers F5/F6, F8/Delete, file-action-menu Copy/Move/Delete, and drag-and-drop drops. Carry it through every rebuild of these variants in the setup-dialog transitions. Leave `active_selection_sources` and job execution unchanged (selection "Operations disclose selected entries hidden by the quick filter"; design D2).
- [ ] 2.4 `crates/filecommand-tui/src/views/delete_confirm.rs`: when `hidden_by_filter > 0`, add a disclosure row between the question and the buttons (`{h} of {n} selected hidden by filter`, or `Selected item is hidden by filter` for one source). Derive the row layout for both `render_delete_confirm` and `hit_buttons` from one shared helper, so the button rects move with the extra row (design D3).
- [ ] 2.5 `crates/filecommand-tui/src/views/destination_input.rs`: when `hidden_by_filter > 0`, draw the same wording on the blank row below the field, truncated to the inner width. Leave the box size, field, caret, title, and drop buttons unchanged (design D3).

## 3. Wildcard matcher (R-19)

- [ ] 3.1 Rewrite `panel::wildcard_match` as the iterative two-pointer glob over `Vec<char>` with single-star backtracking and per-character case-insensitive comparison, with no recursion and no whole-string lowercasing. `select_matching` and `deselect_matching` keep calling it, so `+` and `-` share it (selection "Wildcard pattern matching for group select and deselect"; design D4).

## 4. Tests

- [ ] 4.1 `panel.rs`:
  - a drive-root-style listing (no `..`) with a filter matching nothing makes `selected()` return `None`;
  - with a matching filter, `selected()` returns the cursor entry;
  - with no filter, it returns the cursor entry exactly as before (quick-filter "No visible entry at a drive root means no current entry").
- [ ] 4.2 `panel.rs`: Ins with no current entry leaves `selected` unchanged (R-17 regression: today it toggles the hidden cursor entry).
- [ ] 4.3 `update/tests.rs` (FakeFs root listing without `..`, filter matching nothing, empty selection): F8, F5, F6, F3, F4, Enter, and Ctrl+Enter each produce no effects and no phase change, `file_action_menu` stays `None`, and the command line is unchanged (quick-filter "Cursor-targeted operations do nothing while no entry is visible").
- [ ] 4.4 `update/tests.rs`: after Backspace widens the pattern to match `notes.txt`, F3 emits `Effect::OpenViewer` for `notes.txt` (quick-filter "Widening the pattern restores a current entry").
- [ ] 4.5 `update/tests.rs`:
  - with 5 selected and 2 hidden, F8 yields `DeleteConfirm { hidden_by_filter: 2, sources.len() == 5 }`, and F5 yields `DestinationInput { hidden_by_filter: 2 }`;
  - after confirming, the job's sources are all five;
  - with no filter the field is 0;
  - a drop from the inactive panel computes the count against the source panel;
  - a right-click selection-scoped file-action-menu Delete reports the same count, and an Enter-opened (single-target) menu reports 0;
  - a command-line `del` of an unselected entry hidden by the filter reports 0 (selection "Delete confirmation states the hidden count", "Destination dialog states the hidden count"; design D2).
- [ ] 4.6 `views/delete_confirm.rs` and `views/destination_input.rs`:
  - insta snapshots with `hidden_by_filter = 2` and `= 1` (single source);
  - existing snapshots with `hidden_by_filter = 0` unchanged;
  - a `hit_buttons` test confirming the `(Y)es`/`(N)o` rects sit on the shifted row when the disclosure is shown (selection "No hidden selection leaves the dialogs unchanged").
- [ ] 4.7 `panel.rs` `wildcard_match`:
  - `?.txt` matches `é.txt` and `日.txt`, and does not match `ab.txt`;
  - `*.TXT` matches `readme.txt`;
  - `a*` matches `a`;
  - `*` matches the empty string;
  - `?` does not match the empty string;
  - the existing `wildcard_match_supports_star_and_question_case_insensitive` cases still pass (selection "Question mark matches one multi-byte character", "Matching ignores case").
- [ ] 4.8 `panel.rs`: a pathological case, `*a*a*a*a*a*a*a*a*a*a*b` against a 200-`a` name, returns `false`, and the 1,000-name loop finishes within a generous time bound (selection "Pathological pattern completes promptly").
- [ ] 4.9 `panel.rs`: a proptest comparing `wildcard_match` with a simple reference matcher (recursive over `char`s, small inputs only) for ASCII and non-ASCII strings with `*`/`?`.
- [ ] 4.10 `panel.rs`/`update/tests.rs`: `-` with `?.txt` deselects `é.txt` (selection "Deselect uses the same matching").

## 5. Verify

- [ ] 5.1 `cargo test --workspace` green, including `cargo insta test` review of the new snapshots.
- [ ] 5.2 `cargo clippy --workspace --all-targets` with no new warnings.
- [ ] 5.3 `openspec validate filter-selection-safety --strict` clean.
- [ ] 5.4 Manual check:
  - at `C:\`, Ctrl+P, type `zzzz`, then press F8, F3, and Enter; nothing happens;
  - select several files, filter some out, and press F8 and F5; both dialogs show `N of M selected hidden by filter`;
  - `+` with `?.txt` selects `é.txt`.
