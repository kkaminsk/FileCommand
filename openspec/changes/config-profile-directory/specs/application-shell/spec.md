## ADDED Requirements

### Requirement: Configuration profile directory

The system SHALL resolve one profile directory at startup and SHALL read and write `config.toml`, `history.json`, and `usermenu.toml` only there, never relative to the process working directory. Resolution order SHALL be: the `--config-dir <path>` launch flag; else, when a file named `portable` exists beside the executable, the executable's directory; else the platform default — `%APPDATA%\FileCommand` on Windows and `$XDG_CONFIG_HOME/filecommand` (falling back to `~/.config/filecommand`) elsewhere. The directory SHALL be created on first run. If it cannot be created or written, the session SHALL run on in-memory defaults, SHALL skip persistence, and SHALL raise the dismissable startup-warning dialog naming the path and the error. When the resolved profile holds no `config.toml` and the working directory contains any of the three files, the startup-warning dialog SHALL state where FileCommand now reads its files and that the old files may be moved; the system SHALL NOT copy, move, or delete them. The `--help` usage text SHALL list `--config-dir` and SHALL print the resolved default profile directory.

#### Scenario: Platform default is used when nothing overrides it
- **WHEN** FileCommand starts on Windows from `C:\Projects\app` with no `--config-dir` flag and no `portable` marker beside the executable
- **THEN** it reads and writes `config.toml`, `history.json`, and `usermenu.toml` under `%APPDATA%\FileCommand`, and no file is created in `C:\Projects\app`

#### Scenario: Explicit --config-dir override
- **WHEN** FileCommand is launched with `--config-dir D:\fc-test`
- **THEN** every configuration file is read from and written to `D:\fc-test`, and the portable marker and platform default are ignored

#### Scenario: Portable mode via marker file
- **WHEN** a file named `portable` exists in the executable's directory and no `--config-dir` flag is given
- **THEN** the executable's directory is the profile directory

#### Scenario: History and frecency are shared across launch directories
- **WHEN** the user visits directories in a session launched from `C:\a`, exits, and relaunches from `C:\b`
- **THEN** the Ctrl+J fuzzy-jump list and command history include the entries recorded in the first session

#### Scenario: Unwritable profile falls back without losing the session
- **WHEN** the profile directory cannot be created
- **THEN** the application starts with default settings, the startup-warning dialog names the path and error, and theme/split/history changes made during the session are not written to disk

#### Scenario: Legacy files in the working directory produce a notice, not a migration
- **WHEN** the profile directory has no `config.toml` and the working directory contains `config.toml` and `history.json`
- **THEN** the startup-warning dialog names those files, the working directory, and the profile directory, and both files are left exactly as they were

#### Scenario: Usage names the profile directory
- **WHEN** `filecommand --help` is run
- **THEN** the usage text lists `--config-dir <path>` and ends with the resolved default profile directory path
