## MODIFIED Requirements

### Requirement: Resize handling with 60x16 hard floor and placeholder

The system SHALL reflow the UI on terminal resize events, laying out the interface at any size at or above the 60x16 hard floor, and MUST draw a `screen.placeholder` "terminal too small" message instead whenever the terminal is below that floor, using a single size check that governs both normal and splash states. The placeholder message SHALL name the floor: "resize to at least 60x16". Between the floor and the 80x24 nominal size the interface renders in the degraded forms defined by the `responsive-layout` capability; at or above 80x24 it renders at full fidelity.

The placeholder SHALL be drawn over the active screen without discarding it. While the terminal is below the floor, the following SHALL persist unchanged and SHALL reappear when the terminal is enlarged to at or above the floor:

- the panels and their tabs, cursor, selection, quick filter, and command-line text;
- the built-in editor: its buffer including unsaved edits, caret position, insert/overwrite mode, and modified state;
- the F3 viewer: its file, position, and text/hex and wrap modes;
- a file-operation setup dialog with its typed input;
- a running file-operation job, with its progress, overwrite-conflict, or error-recovery dialog;
- the end-of-job skipped-files summary;
- any open pull-down menu, dialog, or overlay.

The startup splash is the only exception: it SHALL be dismissed when the terminal shrinks below the floor and SHALL NOT return when the terminal is enlarged.

Worker results SHALL continue to be applied while the terminal is below the floor. These include directory-listing events, file-operation progress, conflict, error, and completion events, and replies to requests made before the shrink. A job that completes while the placeholder is shown therefore still re-reads the affected panels, and a conflict or error raised while it is shown waits for the user's answer.

While the terminal is below the floor, input SHALL NOT reach the screen hidden under the placeholder:

- Mouse input SHALL be ignored, except that while the quit-confirmation dialog is open over the placeholder its buttons SHALL stay clickable.
- F10 and Esc SHALL open the quit-confirmation dialog, which is drawn over the placeholder. The exception is when the built-in editor is the hidden screen: there F10 and Esc SHALL be ignored, so unsaved edits are never discarded without the editor's save-on-exit prompt.
- While the quit-confirmation dialog or the startup-warning dialog is open over the placeholder, that dialog's own keys SHALL work as usual.
- Every other key SHALL be ignored.

While the terminal is below the floor, the screen under the placeholder does not own input, so these rules take precedence over that screen's own key handling, including Esc's cancel/dismiss meaning in a hidden menu or dialog under "Quit request keys and confirmation".

#### Scenario: Reflow at or above the floor

- **WHEN** the terminal is resized to a size at or above 60x16
- **THEN** the UI reflows and lays out its regions to the new dimensions

#### Scenario: Degraded band renders panels, not the placeholder

- **WHEN** the terminal is 70x20 — below the 80x24 nominal size but at or above the floor
- **THEN** the panels, command line, and key bar render in their degraded forms and the placeholder is not shown

#### Scenario: Placeholder below the floor

- **WHEN** the terminal is below 60 columns or below 16 rows
- **THEN** the "terminal too small" placeholder message is drawn instead of the normal layout, naming the 60x16 floor

#### Scenario: Shrinking below the floor during splash

- **WHEN** the terminal shrinks below 60x16 while the startup splash is showing
- **THEN** the placeholder replaces the splash, and the splash does not return when the terminal is enlarged again

#### Scenario: Recovery when resized back up

- **WHEN** the terminal is enlarged from below the floor back to at or above 60x16
- **THEN** the normal layout is drawn again in place of the placeholder

#### Scenario: Unsaved editor buffer survives a shrink

- **WHEN** the built-in editor has unsaved edits with the caret on line 12 in overwrite mode, and the terminal shrinks to 50x12 and is then enlarged to 80x24
- **THEN** the editor is shown again with the same buffer contents, the caret on line 12, overwrite mode, and the modified indicator
- **AND** nothing was written to disk and no prompt was raised while the placeholder was shown

#### Scenario: Viewer position and mode survive a shrink

- **WHEN** the F3 viewer shows a file in hex mode scrolled to offset 0x4000, and the terminal shrinks below the floor and is then enlarged
- **THEN** the viewer is shown again on the same file, in hex mode, at offset 0x4000

#### Scenario: Viewer closes normally after a shrink

- **WHEN** the terminal shrinks below the floor and is enlarged while the viewer is open, and the user then closes the viewer
- **THEN** the panels return and the viewer no longer holds the file open

#### Scenario: Running job keeps reporting progress while undersized

- **WHEN** a copy job is running with its progress dialog shown and the terminal shrinks below the floor
- **THEN** the placeholder is shown, the job keeps running, and its progress events keep updating the hidden progress dialog
- **AND** when the terminal is enlarged before the job ends, the progress dialog reappears showing the job's latest progress

#### Scenario: Conflict raised while undersized waits for an answer

- **WHEN** a running copy job reaches an overwrite conflict while the terminal is below the floor
- **THEN** the job waits for an answer and the conflict is not resolved by default
- **AND** when the terminal is enlarged the overwrite-conflict dialog is shown for that file, and answering it resumes the job

#### Scenario: Job finishing while undersized still refreshes the panels

- **WHEN** a copy job completes while the terminal is below the floor
- **THEN** the panels showing the source and destination directories re-read while the placeholder is still shown
- **AND** when the terminal is enlarged the panels show the copied files, or the end-of-job summary when items were skipped

#### Scenario: Open dialog and typed text survive a shrink

- **WHEN** the F5 destination dialog is open with `D:\backup` typed and the terminal shrinks below the floor and is then enlarged
- **THEN** the destination dialog is shown again containing `D:\backup` with the caret where it was

#### Scenario: Typing over the hidden editor does nothing

- **WHEN** the built-in editor is the hidden screen while the terminal is below the floor and the user types letters, presses Enter, F2, F10, or Esc
- **THEN** the buffer, caret, and file on disk are unchanged, no save runs, and no quit-confirmation or save-on-exit dialog opens

#### Scenario: F10 over the placeholder asks to quit

- **WHEN** the panels are the hidden screen while the terminal is below the floor and the user presses F10
- **THEN** the quit-confirmation dialog opens over the placeholder
- **AND** cancelling it leaves the placeholder shown and the hidden panels unchanged

#### Scenario: Mouse input is ignored while undersized

- **WHEN** the terminal is below the floor, no quit-confirmation dialog is open, and the user clicks or turns the mouse wheel anywhere in it
- **THEN** no command reaches the hidden screen and nothing about it changes

#### Scenario: Quit dialog buttons stay clickable over the placeholder

- **WHEN** the quit-confirmation dialog is open over the placeholder and the user clicks its No button
- **THEN** the dialog closes, the placeholder stays shown, and the hidden screen is unchanged
