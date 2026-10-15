## ADDED Requirements

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
