## ADDED Requirements

### Requirement: Help flags print usage and exit before TUI startup

The system SHALL recognise `-h`, `-?`, and `--help` as the sole command-line argument, print a usage block to stdout, and exit with code 0 without acquiring the terminal, alternate screen, raw mode, or mouse capture. The usage block SHALL include: the binary name and version (sourced from `CARGO_PKG_VERSION`), a one-line description, a synopsis line, a flag table listing `-h` / `-?` / `--help`, `--theme <name>`, `--nomouse`, and `--nosplash`, and a line directing the user to press F1 inside the application for the full key reference.

#### Scenario: -h prints usage to stdout and exits 0

- **WHEN** the user runs `filecommand -h`
- **THEN** the usage block is written to stdout, the process exits with code 0, and the terminal is never placed into raw mode or the alternate screen

#### Scenario: -? prints usage to stdout and exits 0

- **WHEN** the user runs `filecommand -?`
- **THEN** the usage block is written to stdout and the process exits with code 0

#### Scenario: --help prints usage to stdout and exits 0

- **WHEN** the user runs `filecommand --help`
- **THEN** the usage block is written to stdout and the process exits with code 0

#### Scenario: Usage block includes version and flag table

- **WHEN** the usage block is printed
- **THEN** it includes the application name, the current version string, a synopsis, entries for `-h` / `-?` / `--help`, `--theme`, `--nomouse`, and `--nosplash`, and a note to press F1 for the full in-app key reference

#### Scenario: Help flag with additional arguments still prints usage

- **WHEN** the user runs `filecommand --help --theme nc-classic`
- **THEN** the usage block is printed and the process exits 0 without launching the TUI

#### Scenario: No help flag — TUI starts normally

- **WHEN** the application is invoked without `-h`, `-?`, or `--help`
- **THEN** no usage text is printed and the TUI starts as normal
