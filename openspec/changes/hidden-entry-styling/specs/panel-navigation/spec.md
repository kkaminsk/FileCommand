## MODIFIED Requirements

### Requirement: Entry row rendering

The system SHALL render entry rows so directories appear in the bright-white directory style and files in the file style, with `▶UP--DIR◀` shown for the `..` entry and `▶SUB-DIR◀` shown in the Size column for directories. Entries flagged hidden — on Windows those carrying the hidden or system attribute, elsewhere names beginning with `.` — SHALL render their name with the `panel.hidden` role instead of the file or directory style; the `..` entry is never hidden. Style precedence SHALL be: drag-target highlight, then cursor bar, then selected, then hidden, then directory/file. The entry under the cursor SHALL render as a full-width inverse bar using the `panel.cursor` role. Rendering SHALL use only ANSI-16 named color roles and CP437 box-drawing/geometric glyphs, with no emoji, Nerd Font, or file-type icons.

#### Scenario: Directory entry styling

- **WHEN** an entry is a directory
- **THEN** its name SHALL render in the `panel.directory` style and its Size column SHALL read `▶SUB-DIR◀`

#### Scenario: Parent entry styling

- **WHEN** the entry list contains the parent `..` entry
- **THEN** it SHALL render as `▶UP--DIR◀` in the `panel.directory` style

#### Scenario: Hidden entries render dimmed

- **WHEN** a directory listing contains `desktop.ini` (hidden attribute) and `.git` (hidden directory)
- **THEN** both names render with the `panel.hidden` role, `.git` still shows `▶SUB-DIR◀`, and neither entry's position in the sorted listing changes

#### Scenario: Selected hidden entry shows selected style

- **WHEN** a hidden file is selected with Ins
- **THEN** its row renders with the `panel.selected` role, not `panel.hidden`

#### Scenario: Cursor bar wins

- **WHEN** the panel cursor is on a hidden entry
- **THEN** that row SHALL render as the full-width `panel.cursor` inverse bar

#### Scenario: Cursor row is an inverse bar

- **WHEN** an entry is under the panel cursor
- **THEN** that row SHALL render as a full-width inverse bar using the `panel.cursor` role
