## ADDED Requirements

### Requirement: Same-directory re-read preserves cursor and scroll position

When a panel re-reads the directory it is already displaying — after a file-operation job completes or is cancelled, on Ctrl+R or the Left/Right menu's Re-read item, or on a stale background tab's activation refresh — the system SHALL restore the cursor to the entry it was on if that entry is still listed; otherwise to the first surviving entry at or after the cursor's previous row; otherwise to the nearest surviving entry before it. The scroll offset SHALL be restored to its previous value and then re-clamped so the window does not extend past the end of the new list and the cursor remains visible. The restored position SHALL hold while the re-read streams in — the cursor SHALL NOT transiently jump to the first entry — and after completion. Navigation into a different directory SHALL continue to reset the cursor and offset to the top. For same-directory re-reads this requirement takes precedence over "Streamed listing keeps the top pinned until the user moves".

#### Scenario: Deleting the cursored file lands on the next entry

- **WHEN** the cursor is on `b.txt` in a listing `a.txt`, `b.txt`, `c.txt` and the user confirms a delete of `b.txt` that completes
- **THEN** after the automatic re-read the cursor is on `c.txt` and the scroll offset is unchanged

#### Scenario: Deleting the last entry lands on the previous one

- **WHEN** the cursor is on the last entry of a listing and the user confirms a delete of that entry that completes
- **THEN** after the re-read the cursor is on the entry that preceded it, now the last entry

#### Scenario: Multi-selection delete lands on the first survivor at or after the cursor

- **WHEN** a listing `a`, `b`, `c`, `d`, `e` has `b`, `c`, `d` selected with the cursor on `c`, and a delete of the selection completes
- **THEN** after the re-read the cursor is on `e`

#### Scenario: Scrolled window stays put

- **WHEN** a 60-entry listing is shown in a 20-row body with scroll offset 30 and the cursor on position 40, and a delete of the cursored entry completes
- **THEN** after the re-read the scroll offset is still 30 and the cursor is on position 40, which is the entry formerly at position 41

#### Scenario: Deleting at the very bottom pulls the window up by one

- **WHEN** a 60-entry listing is shown in a 20-row body with scroll offset 40 and the cursor on the last entry, and a delete of that entry completes
- **THEN** after the re-read the list has 59 entries, the scroll offset is re-clamped to 39, the cursor is on the new last entry, and no blank trailing row is shown

#### Scenario: Ctrl+R keeps the cursor's entry

- **WHEN** the cursor is on `report.txt` partway down a listing and the user presses Ctrl+R
- **THEN** after the re-read the cursor is on `report.txt` and the scroll offset is unchanged

#### Scenario: No jump while the re-read streams

- **WHEN** a same-directory re-read is in progress and its first streamed chunk contains the anchored entry
- **THEN** the cursor is on that entry as soon as the chunk is applied, not on the first entry

#### Scenario: Navigating to a different directory still starts at the top

- **WHEN** the cursor is partway down a listing and the user presses Enter on a subdirectory
- **THEN** the new listing starts with the cursor on its first entry and the scroll offset at 0

## MODIFIED Requirements

### Requirement: Scroll offset is core panel state

Each panel's scroll offset SHALL live in `filecommand-core` panel state (with Tree mode's offset in its tree state), and all offset changes SHALL flow through `core::update` — the renderer only reads it. The offset SHALL be a position in the quick-filter-narrowed visible list, not a raw entry index. Core SHALL derive each panel's body row count from state it already holds (terminal size from `Resize`, the panel split, the panel's display mode, and tab-strip visibility) — it SHALL NOT query the terminal — and SHALL re-clamp the offset so the cursor stays visible after every mutation that can move the cursor or change the list: cursor movement, quick-filter edits, re-sort, streamed listing updates, directory load completion (including find-file's deferred cursor settle and the same-directory re-read anchor restore), tab restore, and terminal resize.

#### Scenario: Quick-filter narrowing re-clamps the offset

- **WHEN** a quick-filter keystroke narrows the visible list so the current offset would leave the cursor's entry outside the window
- **THEN** the offset is re-clamped in the same reducer step so the cursor's entry is visible

#### Scenario: Re-sort keeps the cursor's entry in view

- **WHEN** the sort mode changes and the cursor re-anchors to the same entry at a new position
- **THEN** the offset is re-clamped so that entry is inside the window

#### Scenario: Terminal resize re-clamps

- **WHEN** the terminal shrinks so the current offset would leave the cursor below the new, shorter window
- **THEN** the next reducer step after the `Resize` re-clamps the offset and the cursor is visible in the new geometry

#### Scenario: Tab restore re-clamps against the current viewport

- **WHEN** a tab stashed with a scroll offset is restored while the panel's body height differs from when it was stashed
- **THEN** the restored offset is re-clamped so the restored cursor is visible at the current height

#### Scenario: Streamed listing keeps the top pinned until the user moves

- **WHEN** entries stream into a freshly loaded directory and the user has not moved the cursor
- **THEN** the offset stays 0 with the cursor pinned to the first entry, exactly as the cursor itself already behaves

#### Scenario: Re-read anchor restore re-clamps against the shorter list

- **WHEN** a same-directory re-read restores a scroll offset that would leave blank rows below the end of the now-shorter list
- **THEN** the offset is re-clamped in the same reducer step so the window ends at the last entry and the cursor remains visible
