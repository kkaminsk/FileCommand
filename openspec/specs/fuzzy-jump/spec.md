# fuzzy-jump Specification

## Purpose
TBD - created by archiving change m5-editor-and-modern-extras. Update Purpose after archive.
## Requirements
### Requirement: Fuzzy jump dialog invocation

The system SHALL open the fuzzy jump dialog when the user presses Ctrl+J (the default binding, overridable via `config.toml`), presenting a modal list of previously visited directories drawn in the standard NC dialog style over the panels. The dialog SHALL provide a single-line input field into which the user types a fuzzy match pattern, and Esc SHALL close the dialog without changing the active panel's directory.

#### Scenario: Ctrl+J opens the dialog

- **WHEN** the user presses Ctrl+J while a panel is focused
- **THEN** the fuzzy jump dialog opens as a modal box over the panels with an empty input field and the visited-directory list shown beneath it

#### Scenario: Esc closes without navigating

- **WHEN** the fuzzy jump dialog is open and the user presses Esc
- **THEN** the dialog closes and the active panel remains at its current directory unchanged

#### Scenario: Overridden binding still opens the dialog

- **WHEN** `config.toml` rebinds fuzzy jump to a different key and the user presses that key
- **THEN** the fuzzy jump dialog opens exactly as it would for the default Ctrl+J binding

### Requirement: Fuzzy matching of visited directories

The system SHALL filter the visited-directory list to entries whose path is a subsequence match for the typed pattern, updating the displayed list as each character is typed and as Backspace shortens the pattern. With an empty pattern the system SHALL show the full frecency-ranked list. Matching SHALL be performed in `filecommand-core` independently of the terminal so it is unit-testable.

#### Scenario: Typing narrows to subsequence matches

- **WHEN** the user types a pattern into the dialog input field
- **THEN** the list narrows to only those visited directories whose path contains the pattern's characters as an ordered subsequence

#### Scenario: Backspace re-widens the list

- **WHEN** the user presses Backspace to remove characters from the pattern
- **THEN** the list re-expands to include directories that match the shortened pattern

#### Scenario: Empty pattern shows all entries

- **WHEN** the dialog is open with an empty input field
- **THEN** the full list of visited directories is shown in frecency rank order

### Requirement: Frecency ranking

The system SHALL rank the visited-directory list by a frecency score combining recency and visit frequency, with more recently and more frequently visited directories ranked higher, so the most likely target appears near the top. The ranking computation SHALL live in `filecommand-core` and be deterministic under a pinned clock for testability.

#### Scenario: Frequency raises rank

- **WHEN** two directories match the pattern equally and one has been visited more times
- **THEN** the more frequently visited directory is ranked above the other

#### Scenario: Recency raises rank

- **WHEN** two directories have equal visit frequency and one was visited more recently
- **THEN** the more recently visited directory is ranked above the other

### Requirement: Enter navigates the active panel

The system SHALL, when the user presses Enter on a highlighted directory in the fuzzy jump dialog, close the dialog and navigate the active panel to that directory, leaving the opposite panel unchanged. If the chosen directory no longer exists or cannot be read, the active panel SHALL show its inline panel read-error state rather than crashing.

#### Scenario: Enter jumps the active panel

- **WHEN** the user highlights a directory in the dialog and presses Enter
- **THEN** the dialog closes and the active panel changes its current directory to the selected path while the opposite panel is unaffected

#### Scenario: Chosen directory is missing

- **WHEN** the user presses Enter on a directory that has since been deleted or is inaccessible
- **THEN** the active panel displays its inline read-error state offering re-read/drive change and the application does not crash

### Requirement: Directory history persistence

The system SHALL record each directory the user navigates into in the frecency history and persist that history in `history.json` in the platform config directory, written atomically and shared with command history. The system SHALL load this history on startup so the fuzzy jump dialog reflects directories visited in prior sessions, and SHALL fall back to an empty history when the file is absent or malformed rather than failing to launch.

#### Scenario: Navigation records history

- **WHEN** the user navigates the active panel into a directory
- **THEN** that directory's frecency entry is created or updated with an incremented visit count and refreshed last-visit time

#### Scenario: History survives restart

- **WHEN** the user visits directories, exits FileCommand, and relaunches
- **THEN** the fuzzy jump dialog lists those previously visited directories using their persisted frecency ranking

#### Scenario: Missing or malformed history file

- **WHEN** `history.json` is absent or cannot be parsed at startup
- **THEN** the fuzzy jump history loads as empty and the application launches normally without error

