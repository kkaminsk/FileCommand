# operation-dialogs Delta

## ADDED Requirements

### Requirement: Drop-initiated destination dialog

When a drag-and-drop ends over a valid target, the system SHALL open the destination input dialog with its field pre-filled with the exact drop path and with a button row — `Copy`, `Move`, `Cancel` — which the keyboard-initiated dialog does not have, the focused button being the verb proposed by the drag; the title SHALL name the focused verb and the item count. Activating `Copy` or `Move` SHALL start the corresponding job against the entered destination through the existing overwrite-conflict, progress, error-recovery, and summary flows; `Cancel` or Esc SHALL close the dialog with no effect. Keyboard-initiated F5/F6 dialogs SHALL be unchanged.

#### Scenario: Drop dialog contents

- **WHEN** a plain drag of three entries ends over `D:\BACKUP\OLD`
- **THEN** the dialog is titled `Copy 3 files`, the field reads `D:\BACKUP\OLD`, and `[ Copy ]` is focused

#### Scenario: Switching the verb in the dialog

- **WHEN** the drop dialog is open with `[ Copy ]` focused and the user activates `[ Move ]`
- **THEN** a move job starts against the entered destination

#### Scenario: F5 dialog unchanged

- **WHEN** the user presses F5
- **THEN** the destination dialog renders as today — title, prompt, and field with the opposite panel's path, no button row, Enter confirms and Esc cancels
