## ADDED Requirements

### Requirement: Command history dialog (Alt+F8)

The system SHALL open a modal command-history dialog on Alt+F8 or Commands → History, listing the persisted command history newest first in the primary dialog style. Up/Down/Home/End/PgUp/PgDn SHALL move the highlight; Enter SHALL replace the command-line buffer with the highlighted entry (caret at the end, no history recall in progress) and close the dialog without running anything; Esc SHALL close it unchanged. An empty history SHALL show a single `(no history)` row on which Enter does nothing. The dialog SHALL NOT respond to the mouse (it joins the unsupported-overlay set).

#### Scenario: Pick an entry
- **WHEN** the history holds `cd C:\work` then `cd D:\` and the user presses Alt+F8, Down, Enter
- **THEN** the dialog closes and the buffer reads `cd C:\work` with the caret at the end; nothing has run

#### Scenario: Esc leaves the buffer alone
- **WHEN** the buffer holds `cd x` and the user opens the dialog and presses Esc
- **THEN** the dialog closes and the buffer still reads `cd x`

#### Scenario: Empty history
- **WHEN** the history is empty and the user presses Alt+F8, then Enter
- **THEN** the dialog shows `(no history)` and Enter leaves the buffer unchanged with the dialog open until Esc
