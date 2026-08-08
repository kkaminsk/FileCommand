# user-menu (delta)

## MODIFIED Requirements

### Requirement: Open the F2 user menu

The system SHALL open a modal user menu when F2 is pressed, listing the entries loaded from `usermenu.toml` in file order, each shown by its `label`, followed by a separator row and a built-in `Themes` entry that is always present regardless of the file's content. The menu SHALL render in the primary dialog style (black on cyan, black double-line frame, §4.4) centered over the panels. When `usermenu.toml` contains no entries, F2 SHALL open the menu showing an empty/placeholder list above the separator and built-in entry rather than doing nothing.

#### Scenario: F2 opens the menu with configured entries

- **WHEN** the user presses F2 and `usermenu.toml` defines three entries with labels `Compress`, `Backup`, `Checksum`
- **THEN** a modal user menu opens listing those three labels in file order, followed by a separator row and the built-in `Themes` entry, in the primary dialog style

#### Scenario: F2 with no entries still offers the built-in entry

- **WHEN** the user presses F2 and `usermenu.toml` defines zero entries
- **THEN** the user menu opens showing an empty-state placeholder above the separator and the built-in `Themes` entry, and remains dismissable with Esc

#### Scenario: Only labels are shown in the list

- **WHEN** the user menu is open for an entry whose `label` is `Backup` and whose `command` is `robocopy . D:\backup /E`
- **THEN** the list row displays the label `Backup` and does not display the underlying command string

### Requirement: Navigate and dismiss the user menu

The system SHALL let the user move the highlight with the Up/Down arrow keys across the `usermenu.toml` entries and the built-in `Themes` entry as one continuous list, never highlighting the separator row, and SHALL close the menu without running any command when Esc is pressed. Movement SHALL clamp at the first user entry and at the built-in entry. While the menu is open it is modal: panel and command-line input SHALL NOT be processed until it closes.

#### Scenario: Esc closes the menu without running a command

- **WHEN** the user menu is open and the user presses Esc
- **THEN** the menu closes, no command is dispatched to the shell, and focus returns to the active panel

#### Scenario: Arrow keys move the highlight across both sections

- **WHEN** the user menu lists two `usermenu.toml` entries with the first highlighted and the user presses Down twice
- **THEN** the built-in `Themes` entry is highlighted, the separator was never highlighted, and pressing Down again leaves `Themes` highlighted

## ADDED Requirements

### Requirement: Built-in Themes entry opens the theme selector

The system SHALL, when the user activates the built-in `Themes` entry, close the user menu and open the theme-selection dialog pre-highlighted on the active theme — the same dialog, navigation, immediate-apply, and persistence behavior as opening it from Options → Themes. Cancelling the theme-selection dialog SHALL return to the panels and SHALL NOT reopen the user menu. Activating the built-in entry SHALL NOT dispatch anything to the shell.

#### Scenario: Enter on Themes opens the picker

- **WHEN** the user menu is open with the built-in `Themes` entry highlighted and the user presses Enter
- **THEN** the user menu closes, no shell command is dispatched, and the theme-selection dialog opens with the currently active theme highlighted

#### Scenario: Picker behavior is identical to the Options route

- **WHEN** the theme-selection dialog was opened from the F2 user menu and the user selects a theme with Enter
- **THEN** the theme applies to the whole screen and persists to `config.toml` exactly as when opened from Options → Themes

#### Scenario: Cancelling the picker returns to the panels

- **WHEN** the theme-selection dialog was opened from the F2 user menu and the user presses Esc
- **THEN** the dialog closes leaving theme and configuration untouched, focus returns to the active panel, and the user menu is not reopened
