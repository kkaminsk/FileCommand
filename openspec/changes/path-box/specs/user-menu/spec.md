## MODIFIED Requirements

### Requirement: Navigate and dismiss the user menu

The system SHALL let the user move the highlight with the Up/Down arrow keys across the `usermenu.toml` entries and the built-in `Themes` entry as one continuous list, never highlighting the separator row, and SHALL close the menu without running any command when Esc is pressed. Movement SHALL clamp at the first user entry and at the built-in entry. While the menu is open it is modal: panel and path-box input SHALL NOT be processed until it closes.

#### Scenario: Esc closes the menu without running a command

- **WHEN** the user menu is open and the user presses Esc
- **THEN** the menu closes, no command is dispatched to the shell, and focus returns to the active panel

#### Scenario: Arrow keys move the highlight across both sections

- **WHEN** the user menu lists two `usermenu.toml` entries with the first highlighted and the user presses Down twice
- **THEN** the built-in `Themes` entry is highlighted, the separator was never highlighted, and pressing Down again leaves `Themes` highlighted
