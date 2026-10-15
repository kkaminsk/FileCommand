# Design: full-mode-column-widths

## Context

`panel.rs:555-560`: `interior_w = w - 2`; `reserved_w() = SIZE(9)+DATE(8)+TIME(5)+3 = 25`; `name_w = interior_w - marker_w - 25`. `format_full_row` emits `pad(name,name_w) │ ' ' pad(size,7) ' ' pad(date,7) ' ' pad(time,4)` = `name_w + 22` cells, then the header/row is padded to `interior_w` — three cells wasted. At 80×24 with a 50/50 split, `w = 40`, `interior_w = 38`, `name_w = 13` (12 with the git marker column).

## Goals / Non-Goals

**Goals:**

- Every Size/Date/Time value legible in full at every width where its column renders.
- Keep the ladder, `MIN_NAME_W = 12`, and the 80×24 four-column guarantee.
- NC-style right-aligned sizes.

**Non-Goals:**

- Changing date/time formats, Brief mode, the mini-status, the git marker column, or scrollbar geometry.

## Decisions

### D1: Spend the slack, keep `reserved_w = 25`

New row: `pad(name, name_w)` + `│` + `rpad(size, 9)` + ` ` + `pad(date, 8)` + ` ` + `pad(time, 5)` = `name_w + 25` = exactly the interior. `ColumnSet::reserved_w` is **not** changed: it keeps returning today's 25 / 20 / 12 / 0 (`SIZE 9 + DATE 8 + TIME 5 + 3`, dropping Time then Date), so `choose_columns`, every breakpoint, and its tests hold. Only `format_full_row` changes: the text widths become `SIZE_TEXT_W = 9`, `DATE_TEXT_W = 8`, `TIME_TEXT_W = 5` and the space after `│` goes away. For the reduced sets the row is shorter than the reserved width (Name+Size+Date: `name_w + 19` of 20; Name+Size: `name_w + 10` of 12) and the remaining cells stay trailing padding, exactly as the slack is handled today.

Alternative rejected: widen the columns and let the ladder drop Time at 80×24 — it would break the nominal-size guarantee for no gain, since the slack already covers the content.

### D2: Right-align numeric sizes; directory markers fill the column

`format_size` yields 4 cells in practice (`1023`, `999G`) and at most 5 (`1024K` after rounding), well within 9; right-aligned under a left-aligned `Size` header reads as it did in NC 5.5. `▶SUB-DIR◀`/`▶UP--DIR◀` are 9 cells and fill the column. Header labels (`Name↓`, `Size`, `Date`, `Time`) stay left-aligned in their cells — the sort arrow logic is untouched.

### D3: Snapshots are regenerated, not hand-edited

`cargo insta test --accept` after the change, then review the diff: every Full-mode row should change only in the Size/Date/Time cells and never in the Name cell or the frame.

## Risks / Trade-offs

- [Wide snapshot churn in one commit] → Mechanical and reviewable with the D3 rule.
- [Dropping the space after `│` makes `│▶SUB-DIR◀` adjacent] → That is the NC look and it keeps `name_w` intact at 80 columns.

## Open Questions

- None.
