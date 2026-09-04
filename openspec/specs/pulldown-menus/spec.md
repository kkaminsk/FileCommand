# pulldown-menus Specification

## Purpose
TBD - created by archiving change m3-command-line-and-menus. Update Purpose after archive.
## Requirements
### Requirement: F9 menu-bar overlay

The system SHALL open a full-width menu bar overlaying the top screen row when F9 is pressed, styled black on cyan (`menubar` role), replacing the panels' top borders on that row and hiding the clock while the bar is open. The bar SHALL present exactly five menu titles in order: Left, Files, Commands, Options, Right. Pressing Esc while the bar is open with no pull-down showing SHALL close the bar and restore the top screen row and clock.

#### Scenario: F9 opens the menu bar and hides the clock

- **WHEN** a panel is focused in the normal state and the user presses F9
- **THEN** a single full-width row (black on cyan) overlays the top screen row showing the titles `Left`, `Files`, `Commands`, `Options`, `Right`
- **AND** the clock is not drawn while the bar is open

#### Scenario: Esc closes the bar and restores the top row

- **WHEN** the menu bar is open and no pull-down is showing and the user presses Esc
- **THEN** the menu bar is removed, the panels' top borders reoccupy the top row, and the clock is redrawn

#### Scenario: Menu hotkey letters are highlighted on the bar

- **WHEN** the menu bar is open
- **THEN** each menu title's hotkey letter renders in bright-yellow (`menu.hotkey`) against the cyan bar

### Requirement: Menu title activation and hotkeys

The system SHALL highlight one active menu title white on black (`menu.highlight`) whenever the bar is open, and SHALL open that menu's pull-down. A menu MAY be opened directly by pressing its hotkey letter while the bar is open.

#### Scenario: First menu is active when the bar opens

- **WHEN** the user presses F9 from the normal state
- **THEN** the `Left` menu title is highlighted white on black and its pull-down opens below it

#### Scenario: Hotkey letter jumps to a menu

- **WHEN** the menu bar is open with the `Left` pull-down showing and the user presses the `Commands` hotkey letter
- **THEN** the `Commands` title becomes the active (highlighted) title and its pull-down opens in place of the previous one

### Requirement: Pull-down visuals with separators and disabled items

The system SHALL render an open pull-down as a single-line-framed box (black frame, black text on cyan, `menu.body`) hanging directly below its menu title. The currently selected item SHALL render white on black (`menu.highlight`); disabled items SHALL render grey (white on cyan, `menu.disabled`) and SHALL NOT be selectable; separator rows SHALL be drawn with the `─` glyph. Only CP437-heritage box glyphs SHALL be used for the frame.

#### Scenario: Framed pull-down with a selected item

- **WHEN** a pull-down is open
- **THEN** it is drawn as a single-line-framed box below its title with the selected item highlighted white on black and remaining enabled items black on cyan

#### Scenario: Disabled item is skipped and styled grey

- **WHEN** a pull-down contains a disabled item and the user moves the selection toward it
- **THEN** the disabled item renders grey (white on cyan) and the selection lands on the next enabled item rather than resting on the disabled one

#### Scenario: Separator row rendering

- **WHEN** a pull-down defines a separator between item groups
- **THEN** that row is drawn using the `─` glyph spanning the framed width and is never selectable

### Requirement: Vertical navigation, activation, and dismissal within a pull-down

The system SHALL move the pull-down selection with Up/Down arrows over enabled items only, wrapping or stopping at the ends per NC behavior, activate the selected item with Enter (closing the whole menu overlay), and close the pull-down with Esc while leaving the bar open with its title highlighted.

#### Scenario: Arrow keys move selection over enabled items

- **WHEN** a pull-down is open and the user presses Down
- **THEN** the selection advances to the next enabled item, skipping any disabled item or separator

#### Scenario: Enter activates the selected item and closes the overlay

- **WHEN** an enabled item is selected and the user presses Enter
- **THEN** the item's action is dispatched, the pull-down and menu bar both close, and the top row and clock are restored

#### Scenario: Esc closes the pull-down but keeps the bar

- **WHEN** a pull-down is open and the user presses Esc
- **THEN** the pull-down closes, the menu bar remains open, and the active menu title stays highlighted

### Requirement: Horizontal movement between menus keeps the pull-down open

The system SHALL move the active menu one position with Left/Right arrows while a pull-down is open, closing the current pull-down and opening the adjacent menu's pull-down in a single step so that a pull-down is always showing during horizontal traversal. Movement SHALL wrap from Right to Left and from Left to Right across the five menus.

#### Scenario: Right arrow opens the next menu's pull-down

- **WHEN** the `Files` pull-down is open and the user presses Right
- **THEN** the `Files` pull-down closes, the `Commands` title becomes active, and the `Commands` pull-down opens with no intermediate closed state

#### Scenario: Horizontal movement wraps at the ends

- **WHEN** the `Right` pull-down is open and the user presses Right
- **THEN** the `Left` title becomes active and the `Left` pull-down opens

### Requirement: Menu contents

The system SHALL populate the five menus with their defined items: Left and Right SHALL each offer display mode, sort mode, filter, re-read, drive select, and new/close tab (mirroring each other); Files SHALL offer View, Edit, Copy, Rename/Move, Make directory, Delete, then a separated group Copy to clipboard, Copy path(s), Copy name(s), then Attributes, Select group, Deselect group, Invert, and Quit; Commands SHALL offer Find file, History, Swap panels, Panels on/off, Compare directories, Fuzzy jump, and Menu file edit; Options SHALL offer Configuration, Themes, Editor selection, and Save setup. Items whose backing feature is not yet available SHALL render as disabled entries rather than being omitted.

#### Scenario: Files menu lists its items

- **WHEN** the user opens the `Files` menu
- **THEN** the pull-down lists View, Edit, Copy, Rename/Move, Make directory, Delete, a separator, Copy to clipboard, Copy path(s), Copy name(s), a separator, Attributes, Select group, Deselect group, Invert, and Quit

#### Scenario: Left and Right menus mirror each other

- **WHEN** the user opens the `Left` menu and then the `Right` menu
- **THEN** both list display mode, sort mode, filter, re-read, drive select, and new/close tab item sets

#### Scenario: Not-yet-available feature renders disabled

- **WHEN** the user opens a menu containing an item whose feature is not yet implemented (e.g. Find file or Themes)
- **THEN** that item appears in the pull-down styled grey (white on cyan) and cannot be selected or activated

