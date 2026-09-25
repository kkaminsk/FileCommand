## MODIFIED Requirements

### Requirement: del and rmdir route into the existing delete-confirmation flow

`del <target>` and `rmdir <target>` SHALL resolve `<target>` against the active panel's current directory. `.` and `..` are never valid targets — neither names an entry deletable through these verbs — and SHALL be rejected outright: the panel's error state SHALL be set and no dialog SHALL open. `del` SHALL only accept a target that is a file; `rmdir` SHALL only accept a target that is a directory. A target that doesn't exist, or whose type doesn't match the verb, SHALL be rejected: the panel's error state SHALL be set accordingly and no dialog SHALL open. A valid, type-matched target SHALL open the same delete-confirmation dialog used by F8 and the file-action menu's Delete entry, scoped to that single entry — including the existing non-empty-directory second confirmation for `rmdir` — and SHALL NOT delete anything until that dialog is accepted.

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
