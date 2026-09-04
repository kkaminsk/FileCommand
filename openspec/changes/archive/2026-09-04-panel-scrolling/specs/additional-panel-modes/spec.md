# additional-panel-modes Delta

## ADDED Requirements

### Requirement: Brief mode column scrolling

In Brief display mode the rendered window SHALL be `columns × rows` consecutive positions of the visible entry list, starting at a position that is a whole multiple of the per-column row count, and the window SHALL scroll by whole columns: when a cursor movement would place the cursor past the window's last column the window shifts one column toward the cursor (and symmetrically at the first column), moving the minimum number of columns that restores visibility. Every rendered column SHALL remain full-height and column-aligned after any scroll. Brief mode SHALL show the same overflow-only right-border scrollbar as Full mode, reflecting the cursor window's linear position through the visible list.

#### Scenario: Cursor past the last visible column shifts the window one column

- **WHEN** the cursor sits in the window's last column and a movement steps it past that column's last visible position
- **THEN** the window shifts by exactly one column, the leftmost column's entries leave the window, and the cursor is rendered in the window's last column

#### Scenario: Window start stays on a column boundary

- **WHEN** Brief mode has scrolled any number of times in either direction
- **THEN** the window's first rendered position is a whole multiple of the per-column row count and every rendered column is full-height

#### Scenario: Brief overflow shows the scrollbar

- **WHEN** a Brief-mode panel's visible list holds more entries than `columns × rows`
- **THEN** the body rows of the right border render the `░`/`█` scrollbar in the `panel.scrollbar` role, and when the list fits the border stays unbroken `║`

### Requirement: Tree mode scrolling

In Tree display mode the body SHALL render a window of consecutive rows from the flattened node list, starting at the tree's scroll offset, with the same minimal-shift cursor-follows rules as Full mode — the window moves only when the tree cursor would leave it, by the minimum distance. The `Tree` column-header row SHALL remain fixed while the node rows scroll beneath it. Tree mode SHALL show the same overflow-only right-border scrollbar over its node rows.

#### Scenario: Tree cursor below the bottom scrolls the nodes

- **WHEN** the tree cursor is on the last visible node row and the user presses Down with more nodes below
- **THEN** the node window shifts down one row, the `Tree` header row is unchanged, and the cursor stays on the last visible node row

#### Scenario: Expanding a directory can overflow and shows the scrollbar

- **WHEN** expanding a tree directory grows the flattened node list beyond the body's node rows
- **THEN** the node rows of the right border render the `░`/`█` scrollbar, and collapsing back below the body height restores the unbroken `║` border
