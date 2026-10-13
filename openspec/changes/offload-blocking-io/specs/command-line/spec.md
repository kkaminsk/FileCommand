## MODIFIED Requirements

### Requirement: cd navigates the active panel or rejects a nonexistent target

`cd <path>` SHALL resolve `<path>` against the active panel's current directory (supporting `.`, `..`, bare drive letters, UNC paths, and relative paths), producing the normalized absolute path that "cd, del and rmdir targets resolve to a normalized absolute path" defines. Before navigating, the system SHALL verify the resolved target exists and is a directory. If it does, the active panel SHALL navigate to it exactly as today. If it does not — the target doesn't exist, or exists but is not a directory — the system SHALL reject the command: the active panel's `cwd` SHALL be left completely unchanged, no listing read SHALL be attempted, and the panel's error state SHALL be set to indicate the target could not be found.

The check is made in one of two ways:

- When the resolved target is an entry already listed in the active panel, that listing SHALL answer the check immediately.
- Otherwise the check SHALL run asynchronously on that resolved path, off the event-loop thread, through the `\\?\` long-path abstraction, so that targets longer than 260 characters and UNC targets are checked correctly. While it is outstanding, input and redraw SHALL continue and the panel's status line SHALL show `Checking <path>…`.

An asynchronous answer SHALL be applied only if both of these hold:

- it is still the most recent command-line check;
- the panel the command was entered for still shows the directory it showed when the command was entered.

Otherwise it SHALL be discarded with no navigation and no error message.

#### Scenario: cd navigates to a valid relative subdirectory
- **WHEN** the active panel is at `C:\NORTON` containing subdirectory `sub`, and the command buffer is `cd sub`
- **THEN** the active panel navigates to `C:\NORTON\sub`

#### Scenario: cd to a nonexistent directory is rejected without navigating
- **WHEN** the active panel is at `C:\NORTON` and the command buffer is `cd \NOSUCHDIR`
- **THEN** the active panel's current directory remains `C:\NORTON`, no listing read is started, and the panel shows an error indicating the target was not found

#### Scenario: cd to a file (not a directory) is rejected
- **WHEN** the active panel is at `C:\NORTON`, `readme.txt` is a file in it, and the command buffer is `cd readme.txt`
- **THEN** the active panel's current directory remains `C:\NORTON` and the panel shows an error indicating the target is not a directory

#### Scenario: A listed subdirectory needs no filesystem check
- **WHEN** the active panel at `C:\NORTON` lists the subdirectory `sub` and the command buffer is `cd sub`
- **THEN** navigation to `C:\NORTON\sub` begins in response to the Enter itself, without waiting for any filesystem check

#### Scenario: cd to an unresponsive UNC path does not freeze the UI
- **WHEN** the active panel is at `C:\NORTON` and the command buffer is `cd \\offline-server\share`, whose server does not respond
- **THEN** while the check is outstanding the UI keeps redrawing and accepting keys, the panel's status line shows `Checking \\offline-server\share…`, and the panel's current directory remains `C:\NORTON`
- **AND** when the check finally fails, the panel shows an error indicating the target was not found and no listing read was started

#### Scenario: cd to a path longer than 260 characters
- **WHEN** the command buffer is `cd` followed by an existing directory whose absolute path exceeds 260 characters and which is not listed in the active panel
- **THEN** the check finds the directory and the active panel navigates to it, rather than reporting it as not found

#### Scenario: A late answer after navigating elsewhere is discarded
- **WHEN** `cd \\slow-server\share` is outstanding and the user presses Enter on the listed directory `docs` before its check completes
- **THEN** the panel shows `docs`, and the late answer for `\\slow-server\share` neither navigates the panel nor sets an error

### Requirement: del and rmdir route into the existing delete-confirmation flow

`del <target>` and `rmdir <target>` SHALL resolve `<target>` against the active panel's current directory, producing the normalized absolute path that "cd, del and rmdir targets resolve to a normalized absolute path" defines. `.` and `..` are never valid targets — neither names an entry deletable through these verbs — and SHALL be rejected outright: the panel's error state SHALL be set and no dialog SHALL open. `del` SHALL only accept a target that is a file; `rmdir` SHALL only accept a target that is a directory. A target that doesn't exist, or whose type doesn't match the verb, SHALL be rejected: the panel's error state SHALL be set accordingly and no dialog SHALL open. A valid, type-matched target SHALL open the same delete-confirmation dialog used by F8 and the file-action menu's Delete entry, scoped to that single entry — including the existing non-empty-directory second confirmation for `rmdir` — and SHALL NOT delete anything until that dialog is accepted.

The existence and type check is made in one of two ways:

- When the resolved target is an entry already listed in the active panel, that listing SHALL answer the check immediately.
- Otherwise the check SHALL run asynchronously on that resolved path, off the event-loop thread, through the `\\?\` long-path abstraction. While it is outstanding, input and redraw SHALL continue and the panel's status line SHALL show `Checking <path>…`.

An asynchronous answer SHALL be applied, opening the dialog or setting the error, only if all of these hold:

- it is still the most recent command-line check;
- the panel the command was entered for still shows the directory it showed when the command was entered;
- the panels still own input, with no menu, dialog, or overlay open and no file operation in progress.

Otherwise it SHALL be discarded and no dialog SHALL open.

#### Scenario: del on a file opens the delete-confirmation dialog
- **WHEN** the active panel is at `C:\NORTON`, `notes.txt` is a file in it, and the command buffer is `del notes.txt`
- **THEN** the delete-confirmation dialog opens naming `notes.txt`, and nothing is deleted until it is accepted

#### Scenario: rmdir on a directory opens the delete-confirmation dialog
- **WHEN** the active panel is at `C:\NORTON`, `docs` is a subdirectory in it, and the command buffer is `rmdir docs`
- **THEN** the delete-confirmation dialog opens naming `docs`, requiring the existing second confirmation if it is non-empty, and nothing is deleted until accepted

#### Scenario: del rejects a directory target
- **WHEN** the active panel is at `C:\NORTON`, `docs` is a subdirectory in it, and the command buffer is `del docs`
- **THEN** the command is rejected, no dialog opens, and the panel shows an error indicating the target is a directory

#### Scenario: rmdir rejects a file target
- **WHEN** the active panel is at `C:\NORTON`, `notes.txt` is a file in it, and the command buffer is `rmdir notes.txt`
- **THEN** the command is rejected, no dialog opens, and the panel shows an error indicating the target is not a directory

#### Scenario: del/rmdir on a nonexistent target is rejected
- **WHEN** the command buffer is `del \NOSUCHFILE.TXT`
- **THEN** the command is rejected, no dialog opens, and the panel shows an error indicating the target was not found

#### Scenario: del/rmdir reject the current and parent directories
- **WHEN** the command buffer is `rmdir ..` (or `del ..`, `rmdir .`, `del .`)
- **THEN** the command is rejected, no dialog opens, and the panel shows an error indicating the target is invalid — the dialog must never offer to delete the panel's current or parent directory

#### Scenario: del on an unlisted target on a slow share does not freeze the UI
- **WHEN** the command buffer is `del \\nas\share\old.log`, which is not listed in the active panel, and the server answers slowly
- **THEN** the UI keeps redrawing and accepting keys while the panel's status line shows `Checking \\nas\share\old.log…`
- **AND** once the check reports a file, the delete-confirmation dialog opens naming `old.log`, and nothing is deleted until it is accepted

#### Scenario: A late answer does not open the dialog over a menu
- **WHEN** `rmdir \\nas\share\tmp` is outstanding and the user opens the F9 menu before its check completes
- **THEN** the menu stays open and the delete-confirmation dialog does not open when the check completes

#### Scenario: del on a path longer than 260 characters
- **WHEN** the command buffer is `del` followed by an existing file whose absolute path exceeds 260 characters and which is not listed in the active panel
- **THEN** the delete-confirmation dialog opens naming that file, rather than the target being reported as not found
