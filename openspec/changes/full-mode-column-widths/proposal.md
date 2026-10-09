# Change: full-mode-column-widths

## Why

Full mode renders every Size, Date, and Time value truncated: the renderer reserves 9/8/5 cells per column but pads the text into 7/7/4 (`views/panel.rs` `format_full_row`: `SIZE_COL_W - 2`, `DATE_COL_W - 1`, `TIME_COL_W - 1`), while the values are 9 (`▶SUB-DIR◀`), 8 (`MM-DD-YY`), and 5 (`HH:MM`) cells wide. The README's own screenshots show `▶SUB-DI`, `06-27-2`, `23:3`. The year digit and the minute digit are always cut off, so the user cannot tell 2025 from 2026 or 23:30 from 23:39 without reading the mini-status. Meanwhile each row leaves three interior cells blank at its right edge.

## What Changes

- Full-mode rows and the header lay out as `Name│Size Date Time` with the value widths equal to their content: Size 9, Date 8, Time 5, single-space separators after Size and Date, no space after the `│`. Reserved width stays 25, so the Name-column ladder, its 12-cell minimum, and the "all four columns at 80×24" guarantee are unchanged, and the row now fills the interior exactly.
- Numeric sizes are right-aligned within the 9-cell Size column (Norton Commander convention); `▶SUB-DIR◀` and `▶UP--DIR◀` fill it. Header labels keep their current left alignment and sort arrows.
- `MM-DD-YY` and `HH:MM` render in full.
- Brief mode, the mini-status ladder, and the column-drop ladder are untouched. Snapshot tests that contain Full-mode rows are re-accepted.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `panel-navigation`: "Full display mode layout" gains the exact column widths and the no-truncation guarantee; "Entry row rendering" states that `▶SUB-DIR◀` renders complete and sizes are right-aligned.

## Impact

- `crates/filecommand-tui/src/views/panel.rs` — `format_full_row` and the three text-width constants; `choose_columns`/`reserved_w` are untouched (25 / 20 / 12 / 0).
- **Overlap:** `hidden-entry-styling` also carries a MODIFIED "Entry row rendering" delta; whichever lands second re-bases its text on the other's merged requirement.
- `crates/filecommand-tui/tests/snapshot_views.rs` snapshots containing Full-mode rows (about half of the 100 `.snap` files) are regenerated and reviewed.
- README screenshots become stale (text only; optional refresh).
- Non-breaking.
