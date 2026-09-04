# panel-navigation Specification

## Purpose
TBD - created by archiving change m1-shell. Update Purpose after archive.
## Requirements
### Requirement: Panel state model

Each panel SHALL maintain its own state consisting of a current directory path, an ordered entry list, a cursor position within that list, and a sort order. Panel state SHALL live in `filecommand-core` with no dependency on the terminal, and all mutations SHALL flow through `core::update`.

#### Scenario: Panel exposes current directory and entries

- **WHEN** a panel has finished reading a directory
- **THEN** its state SHALL expose the current directory path, the list of entries for that directory, and a cursor index into that list

#### Scenario: Entries are ordered by the panel's sort order

- **WHEN** a panel's sort order is Name
- **THEN** entries SHALL be presented sorted by name, with the `..` parent entry (when present) ordered first

#### Scenario: Cursor stays within bounds

- **WHEN** the cursor index would move before the first entry or past the last entry
- **THEN** `core::update` SHALL clamp the cursor to a valid entry index and leave panel state otherwise unchanged

### Requirement: Full display mode layout

The system SHALL render each panel in Full display mode with a double-line border, the current directory path centered in the top border, a column header row of `Name | Size | Date | Time`, entry rows, and a mini-status line inside the bottom border. The active panel's path title SHALL render inverse (black on cyan) and the inactive panel's title SHALL render cyan on blue.

#### Scenario: Active panel title is inverse

- **WHEN** a panel is the active panel
- **THEN** its centered path title in the top border SHALL be rendered with the `panel.title.active` role (black on cyan)

#### Scenario: Inactive panel title is not inverse

- **WHEN** a panel is not the active panel
- **THEN** its centered path title SHALL be rendered with the `panel.title.inactive` role (cyan on blue)

#### Scenario: Sort column shows an indicator

- **WHEN** Full mode renders the column header row and the panel is sorted by Name
- **THEN** the header SHALL show a `↓`/`↑` sort indicator next to the active sort column's label

### Requirement: Entry row rendering

The system SHALL render entry rows so directories appear in the bright-white directory style and files in the file style, with `▶UP--DIR◀` shown for the `..` entry and `▶SUB-DIR◀` shown in the Size column for directories. The entry under the cursor SHALL render as a full-width inverse bar using the `panel.cursor` role. Rendering SHALL use only ANSI-16 named color roles and CP437 box-drawing/geometric glyphs, with no emoji, Nerd Font, or file-type icons.

#### Scenario: Directory entry styling

- **WHEN** an entry is a directory
- **THEN** its name SHALL render in the `panel.directory` style and its Size column SHALL read `▶SUB-DIR◀`

#### Scenario: Parent entry styling

- **WHEN** the entry list contains the parent `..` entry
- **THEN** it SHALL render as `▶UP--DIR◀` in the `panel.directory` style

#### Scenario: Cursor row is an inverse bar

- **WHEN** an entry is under the panel cursor
- **THEN** that row SHALL render as a full-width inverse bar using the `panel.cursor` role

### Requirement: Cursor movement

The system SHALL move the active panel's cursor in response to movement commands (up, down, and page/home/end movement) without leaving the current directory, updating the mini-status line to reflect the newly highlighted entry.

#### Scenario: Move cursor down

- **WHEN** the active panel has focus and a move-down command is issued
- **THEN** the cursor SHALL advance to the next entry and the panel SHALL remain in the same directory

#### Scenario: Mini-status reflects highlighted entry

- **WHEN** the cursor moves to a completed-listing entry
- **THEN** the mini-status line SHALL show that entry's name, size, date, and time

### Requirement: Tab switches the active panel

The system SHALL treat exactly one panel as active at a time, and the Tab key SHALL move focus to the other panel. Switching the active panel SHALL update which panel's title renders inverse and which panel receives subsequent movement and navigation commands.

#### Scenario: Tab moves focus to the other panel

- **WHEN** the left panel is active and Tab is pressed
- **THEN** the right panel SHALL become active and its path title SHALL render inverse while the left panel's title reverts to the inactive style

#### Scenario: Subsequent commands target the newly active panel

- **WHEN** the active panel has just changed via Tab
- **THEN** subsequent cursor-movement and Enter/parent-navigation commands SHALL apply to the newly active panel

### Requirement: Enter descends into a directory

When the cursor is on a directory entry, the Enter key SHALL make the active panel navigate into that directory: its current directory becomes the selected directory, a new listing is started for it, and the cursor resets to the first entry.

#### Scenario: Enter on a subdirectory

- **WHEN** the active panel's cursor is on a subdirectory and Enter is pressed
- **THEN** the panel's current directory SHALL become that subdirectory and a listing of its contents SHALL begin

#### Scenario: Enter on the parent entry

- **WHEN** the active panel's cursor is on the `..` entry and Enter is pressed
- **THEN** the panel SHALL navigate to the parent directory

#### Scenario: Cursor resets on descend

- **WHEN** the active panel navigates into a new directory
- **THEN** the cursor SHALL be positioned on the first entry of the new listing

### Requirement: Parent-directory navigation

The system SHALL navigate the active panel to its parent directory when Ctrl+PgUp is pressed, and when Backspace is pressed while the command line is empty. When the current directory is already a filesystem root with no parent, the navigation SHALL be a no-op.

#### Scenario: Ctrl+PgUp goes to the parent

- **WHEN** the active panel is in a non-root directory and Ctrl+PgUp is pressed
- **THEN** the panel SHALL navigate to its parent directory and begin listing it

#### Scenario: Backspace on empty command line goes to the parent

- **WHEN** the command line buffer is empty and Backspace is pressed
- **THEN** the active panel SHALL navigate to its parent directory

#### Scenario: Parent navigation at a root is a no-op

- **WHEN** the active panel is at a filesystem root with no parent and a parent-navigation command is issued
- **THEN** the panel SHALL remain in the current directory unchanged

### Requirement: Streaming listing mini-status

While a directory listing is still being read, the mini-status line SHALL show `Reading… N` with a running, comma-grouped entry count that updates as chunks arrive, and SHALL revert to the normal name/size/date/time line when the read completes. Entries SHALL be inserted in sorted position as they arrive, and the panel SHALL remain interactive while the listing is incomplete, with the cursor held on the first row until the user moves it.

#### Scenario: Mini-status shows running count while reading

- **WHEN** a directory listing is incomplete and 12345 entries have arrived
- **THEN** the mini-status line SHALL read `Reading… 12,345`

#### Scenario: Mini-status reverts on completion

- **WHEN** a directory listing completes
- **THEN** the mini-status line SHALL revert to showing the highlighted entry's name, size, date, and time

#### Scenario: Cursor holds on first row during streaming

- **WHEN** entries continue to arrive and the user has not moved the cursor
- **THEN** the cursor SHALL remain on the first row while newly arriving entries are inserted in sorted position

#### Scenario: Partial listing is interactive

- **WHEN** a directory listing is still streaming
- **THEN** cursor-movement commands SHALL operate on the entries already received without waiting for the read to complete

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

