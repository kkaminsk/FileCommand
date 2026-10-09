## MODIFIED Requirements

### Requirement: Menu contents

The system SHALL populate the five menus with their defined items: Left and Right SHALL each offer Brief, Full, Tree, Quick view, Info, a separator, Name, Extension, Modif. time, Size, Unsorted, a separator, Filter, Re-read, Drive select, a separator, New tab, Close tab (mirroring each other); Files SHALL offer View, Edit, Copy, Rename/Move, Make directory, Delete, a separator, Copy to clipboard, Copy path(s), Copy name(s), a separator, Select group, Deselect group, Invert selection, a separator, Quit; Commands SHALL offer Find file, History, Swap panels, Panels on/off, Fuzzy jump, Menu file edit; Options SHALL offer Themes. Every listed item SHALL be enabled; the disabled-item rendering rule ("Pull-down visuals with separators and disabled items") remains available for future items but no shipped item uses it.

#### Scenario: Files menu lists its items

- **WHEN** the user opens the `Files` menu
- **THEN** the pull-down lists View, Edit, Copy, Rename/Move, Make directory, Delete, a separator, Copy to clipboard, Copy path(s), Copy name(s), a separator, Select group, Deselect group, Invert selection, a separator, and Quit, all enabled

#### Scenario: Left and Right menus mirror each other

- **WHEN** the user opens the `Left` menu and then the `Right` menu
- **THEN** both list the same display-mode, sort-mode, Filter, Re-read, Drive select, New tab, and Close tab items, all enabled

#### Scenario: Commands and Options menus

- **WHEN** the user opens `Commands` and then `Options`
- **THEN** Commands lists Find file, History, Swap panels, Panels on/off, Fuzzy jump, Menu file edit; Options lists Themes; none is disabled

#### Scenario: Removed items are absent

- **WHEN** the user opens every menu
- **THEN** no menu lists Attributes, Compare directories, Configuration, Editor selection, or Save setup

## ADDED Requirements

### Requirement: Every listed item is wired

Activating a pull-down item SHALL perform the same action as its keyboard equivalent, applied to the menu's target panel (Left/Right menus act on their own panel; Files, Commands, Options act on the active panel): Files → View and Edit SHALL follow the F3 and F4 paths; Left/Right → Filter SHALL start the quick filter on that panel (making it the active panel so typed characters narrow it); Left/Right → New tab and Close tab SHALL open or close a tab on that panel; Commands → History SHALL open the command-history dialog; Commands → Swap panels SHALL swap the panels; Commands → Menu file edit SHALL open the user-menu file in the editor.

#### Scenario: Files → View opens the viewer
- **WHEN** the cursor is on `notes.txt` and the user activates Files → View
- **THEN** the menu closes and the F3 viewer opens on `notes.txt`

#### Scenario: Right → Filter on the inactive panel
- **WHEN** the left panel is active and the user activates Right → Filter
- **THEN** the right panel becomes active with its quick filter open, and subsequent typed characters narrow the right panel

#### Scenario: Left → New tab on the inactive panel
- **WHEN** the right panel is active and the user activates Left → New tab
- **THEN** the left panel gains a new tab exactly as Ctrl+T would have created with the left panel active

#### Scenario: Commands → Menu file edit
- **WHEN** the user activates Commands → Menu file edit
- **THEN** the user-menu file opens in the F4 editor path (external editor when configured, else the built-in editor)
