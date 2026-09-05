# application-shell Delta

## MODIFIED Requirements

### Requirement: Terminal ownership and restoration on every exit

The system SHALL acquire the alternate screen, raw mode, and (when enabled) mouse capture on startup and guarantee their release on every exit path — normal quit, error, and panic — via an RAII guard, so the user's terminal is never left in raw mode, on the alternate screen, or with mouse capture active after the process ends. When the TUI is suspended to run a shell command, an external editor, or the scrollback view, mouse capture SHALL be released before the alternate screen is left and re-acquired on resume.

#### Scenario: Terminal acquired on startup

- **WHEN** the application starts
- **THEN** it enters the alternate screen, enables raw mode, and enables mouse capture when configured

#### Scenario: Terminal restored on normal exit

- **WHEN** the application exits normally (for example via F10 quit)
- **THEN** mouse capture is disabled, raw mode is disabled, and the alternate screen is left before the process terminates

#### Scenario: Terminal restored on error exit

- **WHEN** the application exits because of an error after the terminal was acquired
- **THEN** the RAII guard still disables mouse capture and raw mode and leaves the alternate screen

#### Scenario: Suspended shell run gets a normal terminal

- **WHEN** the user runs a command from the command line
- **THEN** mouse capture is released before the child process runs and re-enabled after the TUI resumes

### Requirement: Panic hook restores the terminal before reporting

The system SHALL install a panic hook that disables mouse capture, leaves raw mode, and leaves the alternate screen BEFORE the panic report is printed, and that chains to the previously installed hook so the backtrace still surfaces, so that a panic while in raw mode never leaves the terminal unusable.

#### Scenario: Panic in raw mode restores the terminal first

- **WHEN** a panic occurs while the terminal is in raw mode on the alternate screen with mouse capture enabled
- **THEN** mouse capture is disabled, raw mode is disabled, and the alternate screen is left before any panic report is written

#### Scenario: Original hook still runs

- **WHEN** the panic hook completes its terminal restoration
- **THEN** it delegates to the previously installed hook so the panic message and backtrace are still reported
