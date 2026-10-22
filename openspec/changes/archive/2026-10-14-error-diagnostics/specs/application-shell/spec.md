## ADDED Requirements

### Requirement: Optional diagnostic log

The system SHALL support an opt-in diagnostic log file, chosen at launch by the `--log <path>` flag (also accepted as `--log=<path>`) or by the `FILECOMMAND_LOG` environment variable. When both are given, the flag SHALL win.

When a log path is in effect, the system SHALL append one line per event to that file. Each line SHALL begin with a UTC timestamp, followed by a short category and a message naming the operation and the affected path where there is one. The file SHALL never be truncated. At least the following SHALL be logged, most of which the application otherwise handles without telling the user:

- failures to write `history.json` or `config.toml`;
- results that could not be delivered between a worker thread and the event loop;
- answers that could not be delivered to a running file operation;
- metadata that could not be restored on a copied file;
- every startup warning, including any that a later warning replaced in the startup-warning dialog.

When neither the flag nor the variable is set, the system SHALL NOT create or write any log file.

If the log file cannot be opened for appending, or the flag is given without a path, the session SHALL start without logging and SHALL raise the dismissable startup-warning dialog naming the path and the reason. A failure to write a log line SHALL never interrupt the session.

The `--help` usage block SHALL list `--log <path>` and SHALL mention `FILECOMMAND_LOG`. The log SHALL be implemented without adding a dependency, and `filecommand-core` SHALL NOT write to it directly.

#### Scenario: No log unless asked for

- **WHEN** FileCommand runs without `--log` and with `FILECOMMAND_LOG` unset, and a `history.json` write fails during the session
- **THEN** no log file is created anywhere, and the session continues as before

#### Scenario: A failed history write is logged with a timestamp

- **WHEN** FileCommand is launched with `--log C:\temp\fc.log` and a `history.json` write fails because the profile directory became read-only
- **THEN** `C:\temp\fc.log` gains a line beginning with a UTC timestamp that names the history write and the path of `history.json`, together with the operating-system error
- **AND** no dialog or status-line message is shown for it

#### Scenario: The environment variable enables logging

- **WHEN** `FILECOMMAND_LOG` is set to `D:\logs\fc.log` and FileCommand is launched without `--log`
- **THEN** diagnostic lines are appended to `D:\logs\fc.log`

#### Scenario: The flag wins over the environment variable

- **WHEN** `FILECOMMAND_LOG` is `D:\logs\env.log` and FileCommand is launched with `--log=D:\logs\flag.log`
- **THEN** lines are appended to `D:\logs\flag.log` and `D:\logs\env.log` is not created or written

#### Scenario: Existing log content is kept

- **WHEN** the log file already holds lines from an earlier session
- **THEN** the new session's lines are appended after them

#### Scenario: Unwritable log path warns and continues

- **WHEN** FileCommand is launched with `--log` naming a file in a directory that does not exist
- **THEN** the TUI starts normally, the startup-warning dialog names that path and the reason it could not be opened, and nothing is logged for the session

#### Scenario: Superseded startup warnings are still recorded

- **WHEN** logging is enabled and two startup warnings occur, such as an unknown `--theme` and a malformed `usermenu.toml`, so the dialog shows only the last one
- **THEN** the log holds a line for each of the two warnings

#### Scenario: Usage lists the log options

- **WHEN** the user runs `filecommand --help`
- **THEN** the usage block lists `--log <path>` and mentions the `FILECOMMAND_LOG` environment variable
