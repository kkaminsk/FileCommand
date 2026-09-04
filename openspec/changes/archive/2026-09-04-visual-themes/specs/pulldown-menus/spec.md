# pulldown-menus Specification (delta)

## MODIFIED Requirements

### Requirement: Menu contents

The system SHALL populate the five menus with their defined items: Left and Right SHALL each offer display mode, sort mode, filter, re-read, drive select, and new/close tab (mirroring each other); Files SHALL offer View, Edit, Copy, Rename/Move, Make directory, Delete, Attributes, Select group, Deselect group, Invert, and Quit; Commands SHALL offer Find file, History, Swap panels, Panels on/off, Compare directories, Fuzzy jump, and Menu file edit; Options SHALL offer Configuration, Themes, Editor selection, and Save setup. Activating Options → Themes SHALL open the theme-selection dialog. Items whose backing feature is not yet available SHALL render as disabled entries rather than being omitted.

#### Scenario: Files menu lists its items

- **WHEN** the user opens the `Files` menu
- **THEN** the pull-down lists View, Edit, Copy, Rename/Move, Make directory, Delete, Attributes, Select group, Deselect group, Invert, and Quit

#### Scenario: Left and Right menus mirror each other

- **WHEN** the user opens the `Left` menu and then the `Right` menu
- **THEN** both list display mode, sort mode, filter, re-read, drive select, and new/close tab item sets

#### Scenario: Themes item is enabled and opens the picker

- **WHEN** the user opens the `Options` menu and activates Themes
- **THEN** the menu overlay closes and the theme-selection dialog opens

#### Scenario: Not-yet-available feature renders disabled

- **WHEN** the user opens a menu containing an item whose feature is not yet implemented (e.g. Attributes)
- **THEN** that item appears in the pull-down styled grey (white on cyan) and cannot be selected or activated
