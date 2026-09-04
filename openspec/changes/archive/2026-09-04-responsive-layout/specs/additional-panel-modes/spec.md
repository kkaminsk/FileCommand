# additional-panel-modes Specification (delta)

## MODIFIED Requirements

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
