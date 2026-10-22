## MODIFIED Requirements

### Requirement: Malformed or unrecognized configuration lines produce startup warnings

The system SHALL report each of the following `config.toml` lines as a warning naming the file and the line number, instead of ignoring it silently:

- a line that is neither blank, a comment, a table header, nor `key = value`;
- a quoted value with no valid closing quote;
- an unrecognized key;
- a recognized key placed under a table where it does not belong, such as a `key.*` binding after `[mouse]`. The warning SHALL state that the setting was ignored because top-level settings must come before the first table;
- an unrecognized table header. One warning for the header SHALL cover the keys beneath it;
- a value of `splash`, `animations`, `refresh_interval`, `[mouse] enabled`, or a `key.*` binding that cannot be interpreted. The default for that key SHALL apply.

A well-formed `theme` or `panel_split` line whose value is not a built-in theme name or a valid split percentage SHALL keep its existing silent fallback. A `config.toml` that exists but is not valid UTF-8 SHALL produce a warning, and default settings SHALL apply. A missing `config.toml` SHALL NOT produce a warning.

The warnings that `usermenu.toml` parsing already produces SHALL be reported the same way: an unrecognized key in an entry, a key outside an `[[entry]]` block, and an entry missing `label` or `command`. A `usermenu.toml` that exists but is not valid UTF-8 SHALL be treated as malformed: the startup warning dialog SHALL be raised, the menu SHALL fall back to default entries, and the file SHALL NOT be overwritten.

The warnings of both files SHALL be collected by the shared tokenizer and its parsers. They SHALL be shown once at startup through the dismissable startup-warning dialog as a single summary: the first warning, in file then line order, followed by the count of further warnings. This summary SHALL NOT replace any other startup warning raised in the same startup, such as a profile-directory warning, the `--theme` override warning, or the malformed-`usermenu.toml` warning; when one of those is raised, it SHALL be shown instead of the summary. When `usermenu.toml` is re-read after the user edits it from within FileCommand, its warnings SHALL be shown the same way. Problems in `history.json` SHALL NOT produce warnings. Warnings SHALL NOT prevent the application from launching.

#### Scenario: Binding placed after the mouse table is reported

- **WHEN** `config.toml` contains `[mouse]` on line 8, `enabled = true` on line 9, and `key.fuzzy_jump = "ctrl+g"` on line 11, and the application starts
- **THEN** Ctrl+J still opens the fuzzy jump dialog, and the startup-warning dialog names `config.toml` line 11 and states that `key.fuzzy_jump` is not a `[mouse]` setting and that top-level settings must come before the first table

#### Scenario: Unknown key is reported

- **WHEN** `config.toml` contains `colour = "red"` on line 4 together with `splash = false`, and the application starts
- **THEN** the splash is skipped, and the startup-warning dialog names `config.toml` line 4 and the unknown setting `colour`

#### Scenario: Line without an equals sign is reported

- **WHEN** `config.toml` contains the line `splash false` on line 2
- **THEN** the line is ignored, the splash setting keeps its default, and the startup-warning dialog names `config.toml` line 2

#### Scenario: Uninterpretable boolean is reported

- **WHEN** `config.toml` contains `animations = yes`
- **THEN** animations stay enabled (the default), and the startup-warning dialog names that line and states that the value must be `true` or `false`

#### Scenario: Uninterpretable refresh interval is reported

- **WHEN** `config.toml` contains `refresh_interval = -5`
- **THEN** automatic refresh runs at the default 60-second interval, and the startup-warning dialog names that line and states that the value must be a non-negative whole number of seconds

#### Scenario: Several warnings are summarized

- **WHEN** `config.toml` has three lines that produce warnings and the application starts
- **THEN** the startup-warning dialog shows the warning for the earliest of those lines followed by an indication that 2 more warnings exist

#### Scenario: Clean configuration produces no warning

- **WHEN** `config.toml` and `usermenu.toml` contain only recognized settings, comments, and blank lines
- **THEN** no startup-warning dialog is shown for configuration content

#### Scenario: Unknown theme keeps its silent fallback

- **WHEN** `config.toml` contains `theme = "no-such-theme"`
- **THEN** the active theme is `nc-classic` and no configuration warning is raised for that line

#### Scenario: Unterminated quote is reported

- **WHEN** `config.toml` contains `theme = "purple-lights` on line 3, with no closing quote
- **THEN** the active theme is `nc-classic`, and the startup-warning dialog names `config.toml` line 3

#### Scenario: Non-UTF-8 config.toml is reported

- **WHEN** `config.toml` exists but contains a byte sequence that is not valid UTF-8, such as an ANSI-encoded `é`
- **THEN** the application starts with default settings and the startup-warning dialog states that `config.toml` is not UTF-8 text

#### Scenario: Non-UTF-8 usermenu.toml is kept, not overwritten

- **WHEN** `usermenu.toml` exists but is saved as UTF-16 or contains a byte sequence that is not valid UTF-8, and the application starts
- **THEN** the startup-warning dialog is shown, the F2 menu falls back to the default entries, and `usermenu.toml` on disk is byte-for-byte unchanged

#### Scenario: Unknown user-menu key is shown

- **WHEN** an `[[entry]]` in `usermenu.toml` has `label`, `command`, and an unrecognized key `icon` on line 5, and the application starts
- **THEN** the entry appears in the F2 menu, and the startup-warning dialog names `usermenu.toml` line 5 and the unknown key `icon`

#### Scenario: Warnings after editing the user menu

- **WHEN** the user edits `usermenu.toml` from within FileCommand, adds an entry with an unrecognized key, and returns to the panels
- **THEN** the edited entries replace the F2 menu, and the warning dialog names `usermenu.toml` and the line of the unrecognized key

#### Scenario: Profile warning keeps precedence

- **WHEN** the profile directory cannot be created at startup
- **THEN** the startup-warning dialog shows the unwritable-profile warning, not a configuration warning

#### Scenario: Theme override warning keeps precedence

- **WHEN** `config.toml` contains an unknown setting and the application is launched with `--theme no-such-theme`
- **THEN** the startup-warning dialog shows the `--theme` warning naming `no-such-theme`, not the configuration warning summary
