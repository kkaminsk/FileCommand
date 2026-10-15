## ADDED Requirements

### Requirement: Modal overlays are mutually exclusive by construction

The system SHALL hold the state of the modal overlays in a single overlay slot in `filecommand-core`'s `State`, so that at most one of them is open at any time. The modal overlays are the F9 menu, the drive-select dialog, fuzzy jump, find file, the F2 user menu, the command-history dialog, the theme picker, the file-action menu, the Help window (with its nested About dialog), and the startup warning. Each overlay's handler SHALL receive that overlay's state directly from the slot, so no handler depends on a separately checked gate and none can run while its overlay is closed.

The quit-confirmation dialog is not part of the slot. It SHALL remain a separate layer above every other context, as "Quit request keys and confirmation" requires. The file-operation dialogs, the viewer, and the editor SHALL remain phases.

Which layer owns input SHALL be defined once, by a single precedence order in `filecommand-core`, and the reducer's command routing, the TUI's key routing, its mouse routing, and its rendering and hit-map construction SHALL all follow it. The order, highest first:
1. the quit confirmation;
2. a file-operation dialog;
3. the viewer;
4. the editor;
5. the open overlay;
6. the panels or the startup splash.

A layer SHALL handle only the commands it claims; any other command, including worker replies, SHALL pass to the layers below it as it does today. While the terminal is below the 60x16 floor, input routing SHALL additionally follow "Resize handling with 60x16 hard floor and placeholder".

This structure SHALL NOT change which overlay is shown, which keys or clicks it accepts, or how it renders, in any state the application can reach.

#### Scenario: A menu item opens its dialog in place of the menu

- **WHEN** the F9 menu is open and the user activates Commands → Find file
- **THEN** the menu closes and the find-file dialog opens
- **AND** only the find-file dialog is shown, and it receives the next key

#### Scenario: The user menu's Themes entry replaces the user menu

- **WHEN** the F2 user menu is open with its built-in Themes entry highlighted and the user presses Enter
- **THEN** the user menu closes and the theme picker opens in its place, with the active theme highlighted

#### Scenario: Another overlay's shortcut is ignored while an overlay is open

- **WHEN** the fuzzy-jump dialog is open and the user presses F9 or Alt+F1
- **THEN** neither the menu nor the drive-select dialog opens
- **AND** the fuzzy-jump dialog stays open with its pattern and highlight unchanged

#### Scenario: Unbound keys never reach the panels or the command line

- **WHEN** the F2 user menu is open and the user types a printable character that the user menu does not bind
- **THEN** the key is ignored, so the command line and the panel cursor are unchanged

#### Scenario: Late results of a dismissed search touch no other overlay

- **WHEN** the user dismisses the find-file dialog with Esc while its search is still running, then opens the Help window
- **AND** further matches from the dismissed search arrive
- **THEN** the results are discarded, the Help window is unchanged, and no find-file dialog reappears

#### Scenario: An overlay command with its overlay closed does nothing

- **WHEN** a command that only an overlay binds (for example confirming fuzzy jump) reaches the reducer while that overlay is not open
- **THEN** the state is unchanged, no effect is returned, and nothing panics

#### Scenario: The quit dialog stacks above an open overlay

- **WHEN** the quit-confirmation dialog is open above an open F9 pull-down and the user cancels it
- **THEN** the pull-down is still open exactly as before, and it receives the next key

#### Scenario: Worker replies pass beneath the quit dialog

- **WHEN** a copy job is running, the quit-confirmation dialog is open above its progress dialog, and the job reports progress
- **THEN** the progress dialog underneath shows the new progress and the quit-confirmation dialog stays open

#### Scenario: One precedence order everywhere

- **WHEN** the routing of a key, the routing of a mouse event, and the drawing and hit map of a frame are each computed for the same state
- **THEN** all of them treat the same layer as the one that owns input, the layer defined by the single precedence order
