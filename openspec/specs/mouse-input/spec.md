# mouse-input Specification

## Purpose
TBD - created by archiving change mouse-basics. Update Purpose after archive.
## Requirements
### Requirement: Mouse capture configuration

The system SHALL enable terminal mouse capture by default and SHALL disable it when `config.toml` sets `[mouse] enabled = false` or the process is launched with `--nomouse`. When capture is disabled the application SHALL behave exactly as before this capability existed.

#### Scenario: Default enables capture

- **WHEN** the application starts with no `[mouse]` table in `config.toml`
- **THEN** mouse capture is enabled and clicks are honoured

#### Scenario: Config disables capture

- **WHEN** `config.toml` sets `[mouse] enabled = false`
- **THEN** no mouse-capture sequence is issued and mouse events are not processed

#### Scenario: Flag disables capture

- **WHEN** the application is launched with `--nomouse`
- **THEN** capture is disabled regardless of `config.toml`

### Requirement: Hit-testing stays in the TUI

Each render SHALL record a hit map (panel areas, per-row entry identities keyed by original name, key-bar slots, menu titles and items, dialog buttons) in the TUI crate, and mouse events SHALL be translated there into semantic commands; raw coordinates and terminal modifier types SHALL never be passed into `filecommand-core`.

#### Scenario: Row identity survives scrolling

- **WHEN** the panel is scrolled so `report.txt` is drawn on the third body row and the user clicks that row
- **THEN** the core receives a command naming `report.txt`, not a row index

#### Scenario: Core receives no coordinates

- **WHEN** any mouse event is processed
- **THEN** the command delivered to `core::update` carries entry names, panel sides, slot numbers, or button identities only

### Requirement: Click focuses and places the cursor

A left-click on an entry row SHALL make that panel active and move its cursor to that entry without changing the selection set; a left-click on a panel's title or blank body area SHALL make that panel active only.

#### Scenario: Click on the inactive panel

- **WHEN** the left panel is active and the user clicks `notes.txt` in the right panel
- **THEN** the right panel becomes active with its cursor on `notes.txt`

#### Scenario: Click on a selected entry keeps it selected

- **WHEN** `a.txt` is selected and the user left-clicks it
- **THEN** the cursor moves to `a.txt` and `a.txt` remains selected

### Requirement: Double-click acts as Enter

Two left-clicks on the same entry row within a short interval SHALL behave exactly as pressing Enter on that entry: a directory is entered, `..` goes to the parent, and a file opens the file-action menu.

#### Scenario: Double-click enters a directory

- **WHEN** the user double-clicks `src`
- **THEN** the panel lists `src`

#### Scenario: Double-click on a file opens the action menu

- **WHEN** the user double-clicks `notes.txt`
- **THEN** the file-action menu opens for `notes.txt`

### Requirement: Ctrl+click toggles selection

Ctrl+left-click on an entry row (press and release without movement) SHALL toggle that entry's selection state without advancing the cursor; the parent-directory pseudo-entry SHALL never become selected.

#### Scenario: Toggle on

- **WHEN** `a.txt` is unselected and the user Ctrl+clicks it
- **THEN** `a.txt` is selected and the cursor is on `a.txt`

#### Scenario: Parent entry ignored

- **WHEN** the user Ctrl+clicks `..`
- **THEN** the selection set is unchanged

### Requirement: Wheel moves the cursor of the panel under the pointer

Wheel notches SHALL move the cursor of the panel under the pointer by three rows per notch (whether or not it is the active panel) without changing the active panel; the viewport SHALL follow through the existing scroll-offset rules so the cursor always stays in view. In the viewer a notch SHALL scroll the document three lines through the existing scroll path; in the built-in editor a notch SHALL move the caret three lines.

#### Scenario: Wheel over the inactive panel

- **WHEN** the left panel is active and the user scrolls one notch down over the right panel
- **THEN** the right panel's cursor moves three rows down, its viewport follows if needed, and the left panel remains active

#### Scenario: Wheel in the viewer

- **WHEN** the viewer is open and the user scrolls one notch down
- **THEN** the document scrolls three lines

### Requirement: Key bar, menu bar, pull-down items, and dialog buttons are clickable

A left-click on a function-key-bar slot SHALL dispatch the same command as that function key; a click on a menu-bar title SHALL open that pull-down; a click on an enabled pull-down item SHALL activate it; a click outside an open pull-down SHALL close it; a click on a dialog button SHALL activate that button.

#### Scenario: Key bar Copy

- **WHEN** the user clicks the `5Copy` slot
- **THEN** the destination-input dialog opens exactly as for F5

#### Scenario: Menu item activation

- **WHEN** the user clicks `Files` in the menu bar and then clicks `Delete`
- **THEN** the delete-confirmation flow starts exactly as for F8

#### Scenario: Dialog button

- **WHEN** the overwrite-conflict dialog is open and the user clicks `Skip All`
- **THEN** the choice is applied exactly as if selected by keyboard

### Requirement: Right-click opens the action menu

A right-click on an entry row SHALL move the cursor to that entry and open the file-action menu for it; on a directory the menu SHALL omit View, Edit, and Run; on a selected entry Copy, Move, Delete, and Send to clipboard SHALL act on the selection set.

#### Scenario: Right-click on a file

- **WHEN** the user right-clicks `notes.txt`
- **THEN** the cursor is on `notes.txt` and the file-action menu is open for it

#### Scenario: Right-click on a directory

- **WHEN** the user right-clicks `src`
- **THEN** the action menu opens listing Copy, Rename, Move, Delete, Send to clipboard with no View, Edit, or Run entry

#### Scenario: Right-click on a selected entry

- **WHEN** three entries are selected and the user right-clicks one of them, then activates Copy
- **THEN** the destination-input dialog opens for the three selected entries

### Requirement: Mouse is honoured only where the key would be

Mouse events SHALL be processed over the panels, in an open pull-down, in the open file-action menu (row activation and click-outside-to-close), in modal dialogs (buttons only), and in the viewer/editor (wheel only); all other overlays SHALL ignore mouse events. While a file-operation job is running only a click on Cancel SHALL be honoured.

#### Scenario: Overlay ignores clicks
- **WHEN** the fuzzy-jump dialog is open and the user clicks a panel row
- **THEN** nothing changes

#### Scenario: Running job accepts Cancel only
- **WHEN** a copy job is in progress and the user clicks the progress dialog's Cancel
- **THEN** the job is signalled to cancel, and clicks elsewhere are ignored

### Requirement: Mouse events are coalesced

The event loop SHALL drain all pending mouse events before a redraw, SHALL discard pointer-motion events that carry no button, and SHALL sum consecutive wheel notches, so pointer motion never causes more than one redraw per frame.

#### Scenario: Motion causes no redraw

- **WHEN** the pointer moves across the panels with no button held
- **THEN** no command is dispatched and no redraw is triggered by the motion

### Requirement: File-action menu entries are clickable

A left-click on an enabled file-action-menu row SHALL activate that entry exactly as if it had been highlighted and Enter pressed; a left-click outside the open menu SHALL close it with no action taken, exactly as Esc does. No hover-highlight SHALL follow the pointer while the menu is open — only a click activates or dismisses it.

#### Scenario: Clicking a row activates it
- **WHEN** the file-action menu is open for `notes.txt` and the user clicks the `Edit` row
- **THEN** the menu closes and the F4 edit path opens for `notes.txt`, exactly as if `Edit` had been highlighted and Enter pressed

#### Scenario: Clicking outside the menu closes it with no action
- **WHEN** the file-action menu is open and the user clicks a point outside the menu's rectangle
- **THEN** the menu closes, no action is taken, and the panel cursor and selection are unchanged

#### Scenario: Clicking a different row than the highlighted one activates the clicked row
- **WHEN** the file-action menu is open with `View` highlighted and the user clicks the `Delete` row
- **THEN** `Delete` is activated, not `View`

#### Scenario: No hover-highlight on mouse move
- **WHEN** the file-action menu is open and the pointer moves over a row without a click
- **THEN** the highlighted entry does not change

### Requirement: Click targets match the rendered geometry

Every dialog and menu that accepts clicks SHALL compute its rectangle and the rectangles of its buttons or rows through one layout computation per frame, and its renderer and its hit test SHALL both use that computation. This covers:
- the delete confirmation;
- the drop-initiated destination dialog's button row;
- the quit confirmation;
- the progress dialog;
- the overwrite-conflict dialog;
- the error dialog;
- the skipped-items summary;
- the F9 menu bar and its pull-downs;
- the file-action menu.

The hit map SHALL be recorded by the same dispatch that draws the frame. At every terminal size from the 60x16 floor upward, and after every resize, the cells a click maps to each button or row SHALL be exactly the cells where that button's label or that row is drawn. In a modal dialog a click on any other cell SHALL do nothing, as "Mouse is honoured only where the key would be" already requires; the F9 pull-downs and the file-action menu keep their existing handling of a click that hits none of their titles or rows. The overlay rectangle itself SHALL still come from the unified overlay geometry rule (responsive-layout "Unified overlay geometry").

#### Scenario: Delete confirmation buttons at the size floor

- **WHEN** the delete confirmation for a file with a 200-character name is open at 60x16, with its question truncated with `…`
- **THEN** a click on any cell of the drawn `(Y)es` label confirms exactly as pressing Y, and a click on any cell of the drawn `(N)o` label cancels exactly as pressing N
- **AND** a click on the frame, on the question, or on the spaces between the two labels does nothing

#### Scenario: Quit dialog hotkey cells

- **WHEN** the quit-confirmation dialog is open, at 80x24 or at 60x16
- **THEN** a click on the drawn `Y` of `(Y/N)` confirms quit and a click on the drawn `N` cancels it
- **AND** a click on any other cell of the dialog does nothing

#### Scenario: Progress dialog Cancel follows a resize

- **WHEN** a copy job's progress dialog is shown and the terminal is resized from 120x40 to 80x24
- **THEN** a click on the cells where `[ Cancel ]` is now drawn cancels the job
- **AND** a click on a cell where the button was drawn before the resize, but no longer is, does nothing

#### Scenario: Geometry agrees at every size

- **WHEN** any listed dialog or menu is rendered at any terminal size between 60x16 and 200x60
- **THEN** for each of its buttons or rows, the set of cells the hit map assigns to it equals the set of cells where its label or row is drawn

