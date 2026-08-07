## ADDED Requirements

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
