## MODIFIED Requirements

### Requirement: Full display mode layout

The system SHALL render each panel in Full display mode with a double-line border, the current directory path centered in the top border, a column header row, entry rows, and a mini-status line inside the bottom border. The columns SHALL be the widest set from the ladder `Name+Size+Date+Time → Name+Size+Date → Name+Size → Name` that keeps the Name column at least 12 display cells wide, dropping columns rightmost-first (Time, then Date, then Size) as the panel narrows and restoring them in reverse as it widens; the column header row SHALL show exactly the columns currently rendered. A rendered row SHALL be laid out as the Name cell, a `│` separator, a 9-cell Size value, one space, an 8-cell Date value (`MM-DD-YY`), one space, and a 5-cell Time value (`HH:MM`), so that every rendered Size, Date, and Time value appears untruncated and the row fills the panel interior exactly. At the 80×24 nominal size with the default 50/50 split, all four columns render. The active panel's path title SHALL render inverse (black on cyan) and the inactive panel's title SHALL render cyan on blue.

#### Scenario: All four columns at the nominal size

- **WHEN** a panel renders Full mode at terminal size 80×24 with the default 50/50 split
- **THEN** the header row reads `Name | Size | Date | Time` and all four columns render, with the Name column at least 12 display cells wide

#### Scenario: Values render untruncated at the nominal size

- **WHEN** a panel renders Full mode at 80×24 with the default split and an entry modified on 2026-06-27 at 23:30
- **THEN** the row shows `06-27-26` and `23:30` complete, and a directory row shows `▶SUB-DIR◀` complete, with no blank cells between the Time value and the right border

#### Scenario: Sizes are right-aligned

- **WHEN** two file entries of 87 bytes and 35 KiB render in Full mode
- **THEN** `87` and `35K` are right-aligned within the Size column so their last digits share a cell column

#### Scenario: Time drops first on a narrowing panel

- **WHEN** a panel narrows to where rendering all four columns would leave the Name column under 12 display cells
- **THEN** the Time column and its header are dropped, and the remaining columns render with Name at 12 or more cells

#### Scenario: Name-only at minimum panel width

- **WHEN** a panel is at the 20-column minimum width
- **THEN** only the Name column renders, spanning the panel interior, and the header row shows only `Name`

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

The system SHALL render entry rows so directories appear in the bright-white directory style and files in the file style, with `▶UP--DIR◀` shown for the `..` entry and `▶SUB-DIR◀` shown complete in the Size column for directories; numeric sizes SHALL be right-aligned in that column. The entry under the cursor SHALL render as a full-width inverse bar using the `panel.cursor` role. Rendering SHALL use only ANSI-16 named color roles and CP437 box-drawing/geometric glyphs, with no emoji, Nerd Font, or file-type icons.

#### Scenario: Directory entry styling

- **WHEN** an entry is a directory
- **THEN** its name SHALL render in the `panel.directory` style and its Size column SHALL read `▶SUB-DIR◀` in full

#### Scenario: Parent entry styling

- **WHEN** the entry list contains the parent `..` entry
- **THEN** it SHALL render as `▶UP--DIR◀` in the `panel.directory` style

#### Scenario: Cursor row is an inverse bar

- **WHEN** an entry is under the panel cursor
- **THEN** that row SHALL render as a full-width inverse bar using the `panel.cursor` role
