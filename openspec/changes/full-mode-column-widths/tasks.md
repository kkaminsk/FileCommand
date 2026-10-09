# Tasks: full-mode-column-widths

## 1. Renderer (`crates/filecommand-tui/src/views/panel.rs`)

- [ ] 1.1 Add `SIZE_TEXT_W = 9`, `DATE_TEXT_W = 8`, `TIME_TEXT_W = 5` for the rendered text; leave `ColumnSet::reserved_w` returning today's 25 / 20 / 12 / 0 so `choose_columns` is untouched (design D1)
- [ ] 1.2 `format_full_row`: `│` directly followed by the right-aligned 9-cell size, then ` ` + 8-cell date, then ` ` + 5-cell time (panel-navigation "Full display mode layout": "Values render untruncated at the nominal size"; design D1, D2)
- [ ] 1.3 Add a right-pad helper (or reuse an existing one) for numeric sizes; keep `▶SUB-DIR◀`/`▶UP--DIR◀` left-filled (panel-navigation "Entry row rendering"; design D2)
- [ ] 1.4 Confirm the `choose_columns` unit tests (`choose_columns_drops_time_first` etc.) pass unchanged

## 2. Tests

- [ ] 2.1 `panel.rs` unit test: a 38-cell interior row with a directory entry renders `▶SUB-DIR◀`, `06-27-26`, `23:30` complete and fills exactly 38 cells (scenario "Values render untruncated at the nominal size")
- [ ] 2.2 `panel.rs` unit test: file sizes `87` and `35K` are right-aligned in the Size column (scenario "Sizes are right-aligned")
- [ ] 2.3 `cargo insta test --accept`, then review every changed `.snap`: only Size/Date/Time cells differ (design D3)

## 3. Docs

- [ ] 3.1 README "Panel display modes": note that sizes are right-aligned; optionally refresh `Screen/*.png`

## 4. Verify

- [ ] 4.1 `cargo test --workspace` green; `openspec validate full-mode-column-widths --strict` clean
- [ ] 4.2 Manual at 80×24 and at a wide terminal, with and without a git repository: all four columns, full dates and times, no trailing blank cells
