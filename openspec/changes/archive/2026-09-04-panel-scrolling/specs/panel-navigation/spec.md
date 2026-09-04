# panel-navigation Delta

## ADDED Requirements

### Requirement: Viewport scrolling keeps the cursor visible

In Full display mode the panel body SHALL render a window of consecutive positions from the visible entry list, starting at the panel's scroll offset, and the cursor's position SHALL always lie within that window. The window SHALL move only when a cursor movement would otherwise place the cursor outside it, and SHALL then move the minimum distance that restores visibility — single-step movements shift the window by one line, `Home` pins the window to the top of the list, `End` pins it to the bottom, and jump-style cursor changes (paging, type-ahead jump, find-file's cursor settle) land with the cursor inside the window.

#### Scenario: Cursor moving below the bottom edge scrolls one line

- **WHEN** the cursor is on the last visible row of a Full-mode panel whose list extends beyond the window and the user presses Down
- **THEN** the window shifts down by exactly one line, the cursor stays on the last visible row, and the entry above the old window's first row is no longer shown

#### Scenario: Cursor moving above the top edge scrolls one line

- **WHEN** the window starts below the top of the list, the cursor is on the first visible row, and the user presses Up
- **THEN** the window shifts up by exactly one line and the cursor stays on the first visible row

#### Scenario: Window does not move while the cursor is inside it

- **WHEN** the cursor moves between two rows that are both already visible
- **THEN** the scroll offset is unchanged and no other rows enter or leave the window

#### Scenario: Jump movements land the cursor in view

- **WHEN** a type-ahead jump moves the cursor to an entry outside the current window
- **THEN** the scroll offset changes so the cursor's entry is rendered inside the window

#### Scenario: Home and End pin the window

- **WHEN** the user presses Home, and later End, in a list longer than the window
- **THEN** after Home the window starts at the first position with the cursor on it, and after End the window ends at the last position with the cursor on it

### Requirement: Scroll offset is core panel state

Each panel's scroll offset SHALL live in `filecommand-core` panel state (with Tree mode's offset in its tree state), and all offset changes SHALL flow through `core::update` — the renderer only reads it. The offset SHALL be a position in the quick-filter-narrowed visible list, not a raw entry index. Core SHALL derive each panel's body row count from state it already holds (terminal size from `Resize`, the panel split, the panel's display mode, and tab-strip visibility) — it SHALL NOT query the terminal — and SHALL re-clamp the offset so the cursor stays visible after every mutation that can move the cursor or change the list: cursor movement, quick-filter edits, re-sort, streamed listing updates, directory load completion (including find-file's deferred cursor settle), tab restore, and terminal resize.

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

### Requirement: Scrollbar indicator on overflow

When (and only when) a panel's visible list is longer than its body window, the body's entry rows in the panel's right border column SHALL render a vertical scrollbar in place of the double-line `║` glyphs on those rows — this requirement takes precedence, on overflow, over the unbroken double-line border described by "Full display mode layout". The scrollbar SHALL use only CP437 glyphs — `░` for the track and `█` for the thumb — styled by the `panel.scrollbar` theme role, with thumb length proportional to the visible fraction (minimum one cell) and thumb position proportional to the scroll offset, touching the track's top exactly when the offset is 0 and its bottom exactly when the last position is visible. The scrollbar SHALL occupy only body entry rows: never the top border, tab-strip row, column-header row, or the bottom border and its mini-status. When the list fits the window, the right border SHALL render exactly as it does today, byte-identical.

#### Scenario: Overflowing list shows the scrollbar

- **WHEN** a Full-mode panel's visible list has more entries than the body has rows
- **THEN** the body entry rows of the right border render a `░` track with a `█` thumb in the `panel.scrollbar` role, and the border rows above and below the body are unchanged

#### Scenario: Fitting list keeps the plain border

- **WHEN** the visible list fits entirely within the body window
- **THEN** the right border renders as unbroken double-line `║` glyphs, identical to the rendering before this change

#### Scenario: Thumb touches the ends at the extremes

- **WHEN** the offset is 0, and later the window shows the last list position
- **THEN** the thumb's top cell is the track's top cell in the first case and the thumb's bottom cell is the track's bottom cell in the second

#### Scenario: Thumb reflects position mid-list

- **WHEN** the window sits partway through a long list
- **THEN** the thumb sits strictly between the track's ends, at a row proportional to the offset
