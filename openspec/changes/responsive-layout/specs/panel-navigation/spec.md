# panel-navigation Specification (delta)

## MODIFIED Requirements

### Requirement: Full display mode layout

The system SHALL render each panel in Full display mode with a double-line border, the current directory path centered in the top border, a column header row, entry rows, and a mini-status line inside the bottom border. The columns SHALL be the widest set from the ladder `Name+Size+Date+Time → Name+Size+Date → Name+Size → Name` that keeps the Name column at least 12 display cells wide, dropping columns rightmost-first (Time, then Date, then Size) as the panel narrows and restoring them in reverse as it widens; the column header row SHALL show exactly the columns currently rendered. At the 80×24 nominal size with the default 50/50 split, all four columns render. The active panel's path title SHALL render inverse (black on cyan) and the inactive panel's title SHALL render cyan on blue.

#### Scenario: All four columns at the nominal size

- **WHEN** a panel renders Full mode at terminal size 80×24 with the default 50/50 split
- **THEN** the header row reads `Name | Size | Date | Time` and all four columns render, with the Name column at least 12 display cells wide

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
