# Change: filter-selection-safety

## Why

The 2026-10-12 code audit found two ways the quick filter and group selection can act on, or fail to match, entries the user did not expect.

**R-17 (Medium): the quick filter can leave the cursor on a hidden entry, and operations can act on hidden selections.**

- `PanelState::snap_cursor_to_visible` returns early when no entry is visible. At a drive root there is no `..` entry, so a pattern that matches nothing leaves `cursor` on an entry the filter hides.
- `PanelState::selected()` returns `entries[cursor]` without checking the filter. Enter, F3, F4, F5, F6, F8/Delete, and the file-action menu all go through it, and Ins (`toggle_selection_and_advance`) reads `entries[cursor]` directly. Each of them then acts on a file that is not on screen. The confirmation dialog names the file, which limits the damage, but the user is deleting or moving something they cannot see.
- `update::active_selection_sources` builds F5/F6/F8 sources from the whole selection set, including entries the active filter hides. The user decided to keep that behavior, because selecting and then filtering is a legitimate workflow. But nothing tells the user that the operation reaches beyond what is visible.

**R-19 (Low): `wildcard_match` matches bytes and can backtrack exponentially.**

- `panel::wildcard_match` runs on the `as_bytes()` of the lowercased strings, so `?` consumes one UTF-8 byte, not one character. `?.txt` therefore does not match `é.txt`, a two-byte character, in either `+` (select) or `-` (deselect); both use the matcher.
- The recursive `*` arm is the naive exponential algorithm. A pattern such as `*a*a*a*a*a*a*b` against long runs of `a` can stall the reducer, and with it the whole UI.

## What Changes

- **No current entry when the filter hides everything.**
  - When the quick-filter pattern leaves no entry visible (possible only where there is no `..`, such as a drive root), the panel has no current entry.
  - `selected()` returns an entry only when it is visible under the active filter, and Ins goes through the same rule. Every command that acts on the cursor entry does nothing until the pattern changes so that an entry is visible again: Enter, F3, F4, F5/F6/F8/Delete without a selection, Ins, the file-action menu, and Ctrl+Enter/Ctrl+].
  - When the pattern is widened, the cursor snaps to the nearest visible entry, as today.
- **Hidden selections are disclosed.**
  - Copy, Move, and Delete still act on every selected entry, including entries the quick filter hides (user decision).
  - When any of the selected entries are hidden, the delete confirmation dialog and the copy/move destination dialog (keyboard- or drop-initiated) state how many, for example `2 of 5 selected hidden by filter`.
  - Dialogs with no hidden selected entries are unchanged.
- **Character-correct, bounded wildcard matching.**
  - `+` and `-` share one matcher. In it `?` matches exactly one character (one Unicode scalar value, not one byte), `*` matches any run of characters including none, and comparison stays case-insensitive.
  - Matching uses the iterative two-pointer glob algorithm with single-star backtracking. Its worst case is proportional to pattern length × name length, with no exponential blow-up.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `quick-filter`: "Cursor and mini-status behavior under an active filter" gains the no-visible-entry rule: no current entry, and cursor-targeted commands do nothing until an entry is visible again.
- `selection`:
  - ADDED "Operations disclose selected entries hidden by the quick filter": operations act on every selected entry, and the delete confirmation and copy/move destination dialogs state the hidden count;
  - ADDED "Wildcard pattern matching for group select and deselect": `?` is one character, `*` is any run, case-insensitive, bounded time. It applies to both `+` and `-` without rewriting their existing requirements.

## Impact

- **Specs:** `quick-filter` (one MODIFIED requirement), `selection` (two ADDED requirements). "Destination input dialog" in operation-dialogs (in flight in `line-editing-in-text-fields`) and "Delete confirmation dialog" are not modified.
- **Code (core):**
  - `crates/filecommand-core/src/panel.rs`:
    - `selected()` returns `None` when a quick filter is active and the cursor entry does not match it;
    - `toggle_selection_and_advance` uses `selected()`;
    - a new `hidden_selected_count()`, counting the sources that are selected entries of the panel and fail its active filter;
    - `wildcard_match` is rewritten (char slices, iterative two-pointer algorithm, per-character case folding).
  - `crates/filecommand-core/src/fs_ops/dialog.rs`: `FileOpSetup::DestinationInput` and `FileOpSetup::DeleteConfirm` gain a `hidden_by_filter: usize` field.
  - `crates/filecommand-core/src/update.rs`: `enter_file_op_setup_for_sources` and `enter_delete_confirm_for_sources` compute the field from the source panel; the other constructors of these variants carry it through.
- **Code (TUI):**
  - `crates/filecommand-tui/src/views/delete_confirm.rs`: an extra disclosure row when the count is non-zero, with `hit_buttons` geometry kept in step.
  - `crates/filecommand-tui/src/views/destination_input.rs`: the disclosure goes on the existing blank row below the field, so the box geometry is unchanged.
- **Tests:**
  - `panel.rs` unit tests for `selected()` under the filter, Ins with no current entry, and `wildcard_match` (Unicode `?`, pathological patterns, a proptest against a reference matcher);
  - `update/tests.rs` for F3/F4/F5/F8/Enter no-ops and for the disclosure count;
  - insta snapshots of both dialogs with and without hidden selected entries.
- **Compatibility:** no change for unfiltered panels or for dialogs without hidden selections. `+`/`-` patterns now match multi-byte characters with `?` as users expect.

**Audit findings:** R-17, R-19

**Coordination:**
- `streaming-listing-performance` (P-03) caches visible indices. `snap_cursor_to_visible`, `move_cursor_filtered`, and `hidden_selected_count` should read that cache once it exists. The `selected()` visibility check here is a single-entry match, so it does not depend on the cache.
- `line-editing-in-text-fields` (in flight) modifies operation-dialogs "Destination input dialog" and the destination field rendering. This change does not touch that requirement. The disclosure line uses a row the field rendering does not occupy, so the two changes merge without overlap in `destination_input.rs`'s field code.
- `reducer-overlay-structure` (B-08) unifies render and hit-test geometry. Until it lands, `delete_confirm.rs`'s `hit_buttons` must mirror the new disclosure row exactly, and this change keeps the two in step. If it lands first, the disclosure row goes into `delete_confirm`'s shared layout function instead.
- `relative-destination-resolution` (R-06) adds an early return in `confirm_destination_input` that rebuilds `DestinationInput`, and `link-aware-listing` (R-05) edits `enter_delete_confirm_for_sources`. Whichever lands second carries the new `hidden_by_filter` field through those edits. `offload-blocking-io` (R-09) makes `dispatch_delete_builtin` asynchronous; it still opens the dialog through `enter_delete_confirm_for_sources`.
