## MODIFIED Requirements

### Requirement: Menu contents

The system SHALL populate the five menus with their defined items: Left and Right SHALL each offer Brief, Full, Tree, Quick view, Info, a separator, Name, Extension, Modif. time, Size, Unsorted, a separator, Filter, Re-read, Drive select, a separator, New tab, Close tab (mirroring each other); Files SHALL offer View, Edit, Copy, Rename/Move, Make directory, Delete, a separator, Copy to clipboard, Copy path(s), Copy name(s), a separator, Select group, Deselect group, Invert selection, a separator, Quit; Commands SHALL offer Find file, Swap panels, Panels on/off, Fuzzy jump, Menu file edit; Options SHALL offer Themes. Every listed item SHALL be enabled; the disabled-item rendering rule ("Pull-down visuals with separators and disabled items") remains available for future items but no shipped item uses it.

#### Scenario: Files menu lists its items

- **WHEN** the user opens the `Files` menu
- **THEN** the pull-down lists View, Edit, Copy, Rename/Move, Make directory, Delete, a separator, Copy to clipboard, Copy path(s), Copy name(s), a separator, Select group, Deselect group, Invert selection, a separator, and Quit, all enabled

#### Scenario: Left and Right menus mirror each other

- **WHEN** the user opens the `Left` menu and then the `Right` menu
- **THEN** both list the same display-mode, sort-mode, Filter, Re-read, Drive select, New tab, and Close tab items, all enabled

#### Scenario: Commands and Options menus

- **WHEN** the user opens `Commands` and then `Options`
- **THEN** Commands lists Find file, Swap panels, Panels on/off, Fuzzy jump, Menu file edit; Options lists Themes; none is disabled

#### Scenario: Removed items are absent

- **WHEN** the user opens every menu
- **THEN** no menu lists Attributes, Compare directories, Configuration, Editor selection, History, or Save setup
