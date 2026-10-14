## ADDED Requirements

### Requirement: Configuration files tolerate a UTF-8 BOM and inline comments

The system SHALL ignore a leading UTF-8 byte-order mark (`EF BB BF`) when reading `config.toml`, `usermenu.toml`, and `history.json`, so the first line or key is read exactly as if no byte-order mark were present.

In `config.toml` and `usermenu.toml`, a `#` that appears outside a quoted string SHALL begin a comment that extends to the end of the line, on key lines and on table-header lines, with one exception: in an unquoted `config.toml` value, a `#` SHALL begin a comment only when it is the first character of the value or is preceded by whitespace. The comment SHALL be excluded from the key's value. A `#` inside a quoted value SHALL be kept as part of the value. Comment handling SHALL NOT change how the characters of a quoted `config.toml` value are interpreted: its text between the quotes SHALL be taken verbatim, as before. Both files SHALL be tokenized by a single shared line tokenizer in `filecommand-core` that is unit-testable without a terminal.

#### Scenario: BOM-prefixed config.toml reads its first key

- **WHEN** `config.toml` begins with a UTF-8 byte-order mark immediately followed by the line `splash = false`, and the application starts
- **THEN** the splash is skipped, exactly as when the file has no byte-order mark

#### Scenario: BOM-prefixed usermenu.toml loads its entries

- **WHEN** `usermenu.toml` begins with a UTF-8 byte-order mark immediately followed by `[[entry]]`, a `label`, and a `command` line
- **THEN** that entry appears in the F2 user menu and the file is not treated as malformed

#### Scenario: BOM-prefixed history.json loads

- **WHEN** `history.json` begins with a UTF-8 byte-order mark and otherwise holds valid command and directory history
- **THEN** the command history and the fuzzy jump history load as if the byte-order mark were absent

#### Scenario: Inline comment after a boolean value

- **WHEN** `config.toml` contains `splash = false # start straight in the panels`
- **THEN** the splash is skipped and no warning is raised for that line

#### Scenario: Inline comment after a quoted value

- **WHEN** `config.toml` contains `theme = "purple-lights" # favourite`
- **THEN** the application starts with the `purple-lights` theme active

#### Scenario: Hash inside a quoted value is kept

- **WHEN** `config.toml` contains `editor = "C:\Tools\C# Editor\edit.exe"`
- **THEN** the configured external editor command is exactly `C:\Tools\C# Editor\edit.exe`, backslashes included

#### Scenario: Hash inside an unquoted word is kept

- **WHEN** `config.toml` contains `editor = C:\Tools\C#\edit.exe`
- **THEN** the configured external editor command is exactly `C:\Tools\C#\edit.exe`

#### Scenario: Comment after a table header

- **WHEN** `config.toml` contains the line `[mouse] # pointer settings` followed by `enabled = false`
- **THEN** mouse capture is disabled

#### Scenario: Inline comment in usermenu.toml

- **WHEN** an `[[entry]]` in `usermenu.toml` contains `label = "Build" # main build` and `command = "cargo build"`
- **THEN** the user menu lists an entry labelled `Build` that runs `cargo build`, and the file is not treated as malformed

### Requirement: Malformed or unrecognized configuration lines produce startup warnings

The system SHALL report each of the following `config.toml` lines as a warning naming the file and the line number, instead of ignoring it silently:

- a line that is neither blank, a comment, a table header, nor `key = value`;
- a quoted value with no valid closing quote;
- an unrecognized key;
- a recognized key placed under a table where it does not belong, such as a `key.*` binding after `[mouse]`. The warning SHALL state that the setting was ignored because top-level settings must come before the first table;
- an unrecognized table header. One warning for the header SHALL cover the keys beneath it;
- a value of `splash`, `animations`, `[mouse] enabled`, or a `key.*` binding that cannot be interpreted. The default for that key SHALL apply.

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

### Requirement: Persisted configuration writes are durable

Every write the system makes to `config.toml` or `usermenu.toml` SHALL follow the same sequence. This covers the theme and panel-split rewrites, the first-run creation of the default `usermenu.toml`, and any later configuration write. The content SHALL be written to a uniquely named temporary file in the same directory and flushed to stable storage, and only then SHALL that file replace the target, so an interruption at any point leaves the target with either its complete previous content or its complete new content. If the write fails, the target SHALL be left unchanged, the temporary file SHALL be removed, and the session SHALL continue with the in-memory setting in effect.

#### Scenario: Theme write is flushed before it replaces config.toml

- **WHEN** the user applies a theme and `config.toml` is rewritten
- **THEN** the new content is written to a temporary file in the profile directory and flushed to stable storage before it replaces `config.toml`

#### Scenario: Interrupted configuration write leaves a complete file

- **WHEN** the process or the machine stops while `config.toml` is being rewritten
- **THEN** `config.toml` holds either its complete previous content or its complete new content, never a truncated or empty file

#### Scenario: Failed configuration write leaves the file untouched

- **WHEN** rewriting `config.toml` fails, for example because the disk is full
- **THEN** `config.toml` is unchanged, no temporary file remains in the profile directory, and the applied theme stays active for the session

#### Scenario: First-run user menu is created durably

- **WHEN** FileCommand starts with no `usermenu.toml` in the profile directory
- **THEN** the default `usermenu.toml` is written through a flushed temporary file that replaces the target, so a crash during first run can never leave a partial user-menu file

### Requirement: Configuration rewrites keep settings in the top-level table

When the system rewrites a top-level `config.toml` setting (`theme` or `panel_split`), it SHALL replace only an occurrence of that key that appears before the first table header, keeping that line's trailing comment. When no such occurrence exists, it SHALL insert the new line immediately before the first table header, or at the end of the file when the file has no table. A leading UTF-8 byte-order mark SHALL be kept, and every other line SHALL be left byte-for-byte unchanged.

#### Scenario: New key is inserted above a trailing table

- **WHEN** `config.toml` contains only `[mouse]` and `enabled = false`, and the user applies the `purple-lights` theme
- **THEN** the line `theme = "purple-lights"` is written above `[mouse]`, and after a restart the `purple-lights` theme is active and mouse capture is still disabled

#### Scenario: Existing line is replaced with its comment kept

- **WHEN** `config.toml` contains `theme = "nc-classic" # my pick` and the user applies `yellow-storm`
- **THEN** that line becomes `theme = "yellow-storm" # my pick` and every other line is unchanged

#### Scenario: Byte-order mark is preserved by a rewrite

- **WHEN** `config.toml` begins with a UTF-8 byte-order mark followed by `theme = "nc-classic"`, and the user applies `inverted`
- **THEN** the rewritten file still begins with the byte-order mark and contains exactly one `theme` line, `theme = "inverted"`

#### Scenario: Split rewrite follows the same placement

- **WHEN** `config.toml` contains only a `[mouse]` table and the user adjusts the panel split to 60%
- **THEN** `panel_split = 60` is written above `[mouse]`, and after a restart the panels open at that split

### Requirement: History file strings decode without losing entries

When reading `history.json`, the system SHALL decode a `\uXXXX` escape pair that forms a UTF-16 surrogate pair as the single character it encodes. It SHALL decode a lone surrogate escape, or a `\u` escape without four hexadecimal digits, as the replacement character U+FFFD and continue reading. Such an escape SHALL NOT cause any other command or directory entry to be dropped. The system SHALL recognize the `"commands"` and `"directories"` keys only as keys of the top-level object, never as text inside a string value.

#### Scenario: Surrogate pair decodes to one character

- **WHEN** a command in `history.json` is stored as `"echo \uD83D\uDE00"`
- **THEN** the loaded command history contains `echo 😀`

#### Scenario: Lone surrogate does not drop later commands

- **WHEN** `history.json` stores the commands `"cd a"`, `"cd b\uD83D"`, and `"cd c"` in that order
- **THEN** the loaded command history contains three entries: `cd a`, `cd b` followed by U+FFFD, and `cd c`

#### Scenario: Key text inside a string is not mistaken for a key

- **WHEN** the `"commands"` array in `history.json` contains the string `"directories"` followed by the string `"cd C:\\"`, and the file's top-level `"directories"` key holds two directory entries
- **THEN** both directory entries are loaded into the fuzzy jump history
