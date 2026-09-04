## MODIFIED Requirements

### Requirement: Mouse is honoured only where the key would be

Mouse events SHALL be processed over the panels, in an open pull-down, in the open file-action menu (row activation and click-outside-to-close), in modal dialogs (buttons only), and in the viewer/editor (wheel only); all other overlays SHALL ignore mouse events. While a file-operation job is running only a click on Cancel SHALL be honoured.

#### Scenario: Overlay ignores clicks
- **WHEN** the fuzzy-jump dialog is open and the user clicks a panel row
- **THEN** nothing changes

#### Scenario: Running job accepts Cancel only
- **WHEN** a copy job is in progress and the user clicks the progress dialog's Cancel
- **THEN** the job is signalled to cancel, and clicks elsewhere are ignored

## ADDED Requirements

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
