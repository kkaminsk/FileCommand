## MODIFIED Requirements

### Requirement: Fuzzy jump dialog invocation

The system SHALL open the fuzzy jump dialog when the user presses Ctrl+J (the default binding, overridable via `config.toml`), presenting a modal list of previously visited directories drawn in the standard NC dialog style over the panels. The dialog SHALL provide a single-line input field — a shared text field per `text-field-editing` — into which the user types a fuzzy match pattern, and Esc SHALL close the dialog without changing the active panel's directory.

#### Scenario: Ctrl+J opens the dialog

- **WHEN** the user presses Ctrl+J while a panel is focused
- **THEN** the fuzzy jump dialog opens as a modal box over the panels with an empty input field and the visited-directory list shown beneath it

#### Scenario: Esc closes without navigating

- **WHEN** the fuzzy jump dialog is open and the user presses Esc
- **THEN** the dialog closes and the active panel remains at its current directory unchanged

#### Scenario: Pattern edited mid-string re-narrows the list

- **WHEN** the pattern reads `prj` and the user presses Left, then types `o`
- **THEN** the pattern reads `proj` and the list is re-narrowed against `proj`

#### Scenario: Overridden binding still opens the dialog

- **WHEN** `config.toml` rebinds fuzzy jump to a different key and the user presses that key
- **THEN** the fuzzy jump dialog opens exactly as it would for the default Ctrl+J binding
