## ADDED Requirements

### Requirement: Drive-select dialog invocation and enumeration

The system SHALL open a drive-select dialog for the panel bound to the pressed key — Alt+F1 for the left panel, Alt+F2 for the right panel — and SHALL populate it by enumerating the available drive letters synchronously via `GetLogicalDrives`, showing every enumerated drive immediately without waiting on any per-drive metadata query.

#### Scenario: Alt+F1 opens the dialog for the left panel

- **WHEN** the user presses Alt+F1 while either panel is focused
- **THEN** the drive-select dialog opens targeting the left panel
- **AND** it lists every drive letter returned by `GetLogicalDrives` at the moment of opening

#### Scenario: Alt+F2 targets the right panel

- **WHEN** the user presses Alt+F2
- **THEN** the drive-select dialog opens targeting the right panel

#### Scenario: Drive letters appear before any label is known

- **WHEN** the dialog opens
- **THEN** all enumerated drive letters are rendered on the first painted frame
- **AND** no label, free-space, or media probe is performed on the paint or input path before the letters are shown

#### Scenario: Dismissing the dialog

- **WHEN** the user presses Esc while the drive-select dialog is open
- **THEN** the dialog closes and the target panel retains its current directory

### Requirement: Lazy, non-blocking volume-label fetch

The system SHALL fetch each drive's volume label lazily on a worker thread and SHALL render the label column blank for that drive until its fetch resolves, at which point the resolved label MUST replace the blank in place. The dialog MUST NOT block paint or input on media presence or network reachability, so drives with absent media or slow network backing never stall the dialog.

#### Scenario: Label fills in place when it resolves

- **WHEN** a drive's volume-label fetch completes on the worker thread
- **THEN** that drive's label appears in place in the already-open dialog
- **AND** the positions and letters of the other listed drives are unchanged

#### Scenario: Absent media does not stall the dialog

- **WHEN** the dialog lists a drive with no media inserted (for example `A:`)
- **THEN** the dialog remains interactive and fully painted with that drive's label column blank
- **AND** no query for that drive blocks the render or input loop

#### Scenario: Slow network drive stays blank without hanging

- **WHEN** a listed network drive's label fetch has not yet returned
- **THEN** that drive's label column stays blank while the user can still navigate and select other drives

#### Scenario: Stale results are discarded

- **WHEN** a label result arrives for a drive after the dialog for it has closed or been superseded
- **THEN** the result is discarded and does not mutate any current panel or dialog state

### Requirement: Selecting a drive changes the panel or surfaces the error state

The system SHALL, when the user selects a drive and confirms, switch the target panel to that drive's directory; if the selected drive is unavailable (no media, disconnected, or otherwise unreadable), the system SHALL surface the target panel's inline error state rather than hanging.

#### Scenario: Selecting an available drive switches the panel

- **WHEN** the user selects an available drive and presses Enter
- **THEN** the drive-select dialog closes
- **AND** the target panel reads and displays that drive's directory

#### Scenario: Selecting an unavailable drive shows the panel error state

- **WHEN** the user selects a drive that has no media or is unreadable and presses Enter
- **THEN** the target panel enters its inline read-error state
- **AND** the application does not hang or crash

### Requirement: UNC path entry

The system SHALL accept UNC paths (`\\server\share`) as panel targets entered manually, treating a validly reachable UNC path the same as a local directory target and applying the same non-blocking error handling when it is unreachable.

#### Scenario: Manual UNC path opens a share

- **WHEN** the user enters a valid, reachable UNC path as a panel target
- **THEN** the target panel navigates to that share and lists its contents

#### Scenario: Unreachable UNC path yields the panel error state

- **WHEN** the user enters a UNC path that cannot be reached
- **THEN** the target panel surfaces its inline read-error state without hanging the UI
