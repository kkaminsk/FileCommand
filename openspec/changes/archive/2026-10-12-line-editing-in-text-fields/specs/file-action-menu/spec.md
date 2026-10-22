## MODIFIED Requirements

### Requirement: In-place Rename

The Rename menu entry SHALL open an input dialog pre-filled with the target entry's current name, with the caret at the end of the field; the field SHALL be a shared text field per `text-field-editing`. Accepting the dialog SHALL rename the entry within its current directory using the existing same-volume rename machinery, including identity-aware case-only rename. Cancelling with Esc SHALL rename nothing. A rename that collides with an existing name or fails SHALL surface the existing overwrite-conflict or error-recovery dialogs from `operation-dialogs`. On successful rename the affected panel SHALL be re-read automatically.

#### Scenario: Rename pre-fills the current name

- **WHEN** the user activates Rename for `draft.txt`
- **THEN** an input dialog opens pre-filled with `draft.txt` and the caret at the end of the field

#### Scenario: Editing the stem without retyping the extension

- **WHEN** the rename dialog for `draft.txt` is open and the user presses Home, then Ctrl+Right, then types `-v2`
- **THEN** the field reads `draft-v2.txt`

#### Scenario: Accepting renames in place

- **WHEN** the rename dialog for `draft.txt` is accepted with the value `final.txt`
- **THEN** `draft.txt` is renamed to `final.txt` in the same directory and the panel is re-read

#### Scenario: Case-only rename works

- **WHEN** the rename dialog for `readme.md` is accepted with the value `README.md`
- **THEN** the entry's name case changes to `README.md` using the identity-aware case-only rename path

#### Scenario: Esc renames nothing

- **WHEN** the rename dialog is open and the user presses Esc
- **THEN** the dialog closes and the entry keeps its original name
