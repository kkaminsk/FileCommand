# additional-panel-modes Specification

## Purpose
TBD - created by archiving change m5-editor-and-modern-extras. Update Purpose after archive.
## Requirements
### Requirement: Brief display mode

The panel SHALL provide a Brief display mode that lists entry names only, arranged in `max(1, floor(interior_width / 12))` columns across the panel width — where `interior_width` is the panel's width inside its border — with the division remainder given to the last column and no Size/Date/Time columns. At the 80×24 nominal size with the default split this yields exactly three columns. Column alignment MUST use display width (`unicode-width`) so that CJK, emoji, and wide-glyph names stay aligned, and entries MUST retain the standard directory/file/selection/cursor styling and the `▶UP--DIR◀` marker for `..`.

#### Scenario: Brief mode renders three columns at the nominal size

- **WHEN** the panel is in Brief display mode at terminal size 80×24 with the default 50/50 split (interior width 38)
- **THEN** entry names are laid out across exactly three columns of widths 12, 12, and 14, and no size, date, or time fields are shown for any entry

#### Scenario: Wider panels earn more columns

- **WHEN** the panel is in Brief mode with an interior width of 60
- **THEN** entry names are laid out across five columns

#### Scenario: Narrow panels drop to fewer columns

- **WHEN** the panel is in Brief mode with an interior width of 23
- **THEN** entry names are laid out in a single column spanning the interior width

#### Scenario: Brief mode aligns wide-character names by display width

- **WHEN** the panel is in Brief mode and a column contains a name with CJK or emoji characters
- **THEN** the following column starts at the same screen offset for every row, using `unicode-width` display width rather than byte or `char` count

#### Scenario: Brief mode preserves entry styling and parent marker

- **WHEN** the panel is in Brief mode and contains directories, files, a selected entry, and the `..` parent entry
- **THEN** directories render in the directory style, files in the file style, selected entries in the selected style, the cursor as a full-width inverse bar, and `..` renders as `▶UP--DIR◀`

### Requirement: Tree display mode structure and rendering

The panel SHALL provide a Tree display mode that renders a directory tree of the current drive. The column-header row MUST read `Tree`; the first body row MUST be the drive root (e.g. `C:\`) in the directory (bright-white) style; descendants MUST be drawn one indent level per depth using the single-line branch glyphs `│  `, `├─`, and `└─` in the frame (cyan) style, with directory names shown in bright-white UPPERCASE. The cursor MUST be the standard inverse bar, and the mini-status line MUST show the highlighted directory's full path. No `+`/`-` expander glyphs are rendered in v1.

#### Scenario: Tree header and root row

- **WHEN** the panel enters Tree mode on drive `C:`
- **THEN** the column-header row reads `Tree` and the first body row is `C:\` rendered in the bright-white directory style

#### Scenario: Tree branch glyphs and indentation

- **WHEN** the tree shows directories at multiple depths
- **THEN** each depth is indented one level further and drawn with the `│  `, `├─`, `└─` branch glyphs in the cyan frame style, with directory names in bright-white UPPERCASE

#### Scenario: Tree mini-status shows highlighted path

- **WHEN** the cursor is on a tree node
- **THEN** the mini-status line shows that directory's full path

### Requirement: Tree lazy expansion

The Tree mode SHALL read directory children lazily — a directory's children are read from the filesystem only when that directory is expanded, never by scanning the whole drive up front. A directory that has not yet been expanded MUST simply show no children rather than an expander glyph or a placeholder.

#### Scenario: Children read on expand

- **WHEN** a tree directory is expanded for the first time
- **THEN** its immediate child directories are read at that moment and inserted beneath it

#### Scenario: Unexpanded directory shows no children

- **WHEN** a tree directory has not yet been expanded
- **THEN** it is drawn with no child rows and no `+`/`-` expander glyph

#### Scenario: No up-front full-drive scan

- **WHEN** the panel first enters Tree mode
- **THEN** only the drive root's immediate children are read, and deeper directories are not enumerated until expanded

### Requirement: Tree mode drives the opposite panel

While the panel is in Tree mode, moving the tree cursor SHALL update the opposite panel to list the currently highlighted directory. Pressing Enter SHALL return this panel to its previous (pre-Tree) list display mode, positioned at the highlighted directory.

#### Scenario: Cursor movement updates opposite panel

- **WHEN** the tree cursor moves to a different directory node
- **THEN** the opposite panel re-lists the contents of that highlighted directory

#### Scenario: Enter returns to prior list mode at chosen directory

- **WHEN** the user presses Enter on a highlighted tree node and the panel's previous display mode was Full
- **THEN** this panel leaves Tree mode, returns to Full mode, and shows the chosen directory's contents

### Requirement: Quick View preview of the opposite panel cursor file

The panel SHALL provide a Quick View display mode that previews the file under the *opposite* panel's cursor. The panel's top-border title MUST read `Quick view` (inverse when active); the body MUST render the head of that file exactly like the viewer's text mode — wrap on, lossy UTF-8 decoding, in the `viewer.text` style, with no viewer controls. The mini-status line MUST show the previewed file's name and size. Binary content MUST be shown with lossy replacement characters, with no hex mode available in Quick View.

#### Scenario: Text file preview mirrors viewer text mode

- **WHEN** the opposite panel's cursor is on a text file and this panel is in Quick View mode
- **THEN** the body shows the head of that file with wrap on, lossy UTF-8 decoding, in the `viewer.text` style, and the top-border title reads `Quick view`

#### Scenario: Preview follows the opposite cursor

- **WHEN** the opposite panel's cursor moves to a different file
- **THEN** the Quick View body updates to preview the newly highlighted file and the mini-status shows that file's name and size

#### Scenario: Binary content shown lossily without hex mode

- **WHEN** the opposite panel's cursor is on a binary file
- **THEN** the body renders its head with lossy replacement characters and no hex-mode toggle is offered

### Requirement: Quick View directory indicator

When the opposite panel's cursor is on a directory, the Quick View body SHALL show a centered `▶SUB-DIR◀` indicator in the directory style and render no file preview.

#### Scenario: Directory under opposite cursor

- **WHEN** this panel is in Quick View mode and the opposite panel's cursor is on a directory
- **THEN** the body shows a centered `▶SUB-DIR◀` marker and no file content is previewed

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

