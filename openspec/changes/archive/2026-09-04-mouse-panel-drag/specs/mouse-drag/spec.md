# mouse-drag Delta

## ADDED Requirements

### Requirement: Drag lifecycle

A left- or right-button press on an entry row followed by pointer movement of at least one cell SHALL begin a drag whose items are the panel's selection set when the pressed entry is selected, otherwise the pressed entry alone; the parent-directory pseudo-entry SHALL never be dragged. The items SHALL be captured at drag start. Releasing the button over a valid target SHALL open the drop-initiated destination dialog; releasing anywhere else SHALL end the drag with no effect.

#### Scenario: Drag the selection

- **WHEN** three entries are selected and the user presses on one of them, moves the pointer to the other panel, and releases
- **THEN** the destination dialog opens for the three entries

#### Scenario: Drag an unselected entry

- **WHEN** entries are selected but the user presses on an unselected entry and drags it to the other panel
- **THEN** the destination dialog opens for that single entry and the selection set is unchanged

#### Scenario: Release on an invalid spot

- **WHEN** a drag is in progress and the user releases over the key bar
- **THEN** no dialog opens and nothing changes

### Requirement: Verb selection

A plain or Ctrl-modified left-button drag SHALL propose Copy; a Shift-modified left-button drag or a right-button drag SHALL propose Move. The proposed verb SHALL be recomputed from the modifier flags of each drag and release event and SHALL only determine which button the drop dialog focuses — Ctrl SHALL never propose Move.

#### Scenario: Plain drag proposes Copy

- **WHEN** the user drags with no modifier and releases over the other panel
- **THEN** the drop dialog opens with `[ Copy ]` focused

#### Scenario: Right-button drag proposes Move

- **WHEN** the user drags with the right button and releases over the other panel
- **THEN** the drop dialog opens with `[ Move ]` focused

#### Scenario: Ctrl+drag is Copy

- **WHEN** the user holds Ctrl throughout a left-button drag
- **THEN** the drop dialog opens with `[ Copy ]` focused

### Requirement: Valid drop targets

Valid targets SHALL be: the other panel's current directory (its title, blank body area, or any non-directory row); a subdirectory row or the `..` row in either panel; a node in a Tree-mode panel; a tab in the other panel's tab strip. Info and Quick View panels SHALL never be targets. A target equal to the items' own directory, or equal to or inside a dragged directory, SHALL be invalid.

#### Scenario: Subdirectory row is the target

- **WHEN** the user drags `notes.txt` from `C:\PROJECTS` and releases over the `OLD` row of the right panel showing `D:\BACKUP`
- **THEN** the drop dialog opens pre-filled with `D:\BACKUP\OLD`

#### Scenario: Same-panel subdirectory

- **WHEN** the user drags `notes.txt` onto the `src` row in the same panel
- **THEN** the drop dialog opens pre-filled with `C:\PROJECTS\src`

#### Scenario: Directory onto itself is invalid

- **WHEN** the user drags the `src` directory and hovers over the `src` row or over a listing inside `src`
- **THEN** the mini-status shows `Can't drop here` and releasing does nothing

#### Scenario: Same directory is invalid

- **WHEN** the user drags an entry and releases over the blank area of its own panel
- **THEN** nothing happens

### Requirement: Drag feedback

While a drag is in progress the system SHALL render the current valid target panel's frame and title in the `panel.frame.drop` role, the target row (when the target is a row, node, or tab) in the `button.focused` role, the target panel's mini-status as `<Verb> N file(s) ► <dir>\` (or `Can't drop here` over an invalid target), and the key bar as `Drop=Copy  Shift/RightBtn=Move  Esc=Cancel`. Only CP437-heritage glyphs SHALL be used, and in themes whose frame colour cannot change meaningfully the `panel.frame.drop` role SHALL be an inversion. Source rows SHALL render unchanged. All treatments SHALL end when the drag ends. No feedback SHALL be drawn for pointer motion outside a drag.

#### Scenario: Target panel lights up

- **WHEN** a drag moves over the other panel
- **THEN** that panel's frame and title use `panel.frame.drop`, its mini-status names the verb, count, and directory, and the key bar shows the drag labels

#### Scenario: Feedback ends with the drag

- **WHEN** the drag ends by release or Esc
- **THEN** frames, rows, mini-status, and key bar render exactly as before the drag

### Requirement: Cancel and phase-change clear the drag

Pressing Esc during a drag SHALL cancel it. Any state transition that leaves the panels phase or opens a menu or overlay (job completion, listing failure, F9, quit request, resize below the minimum) SHALL clear the drag state so no drop can complete afterwards.

#### Scenario: Esc cancels

- **WHEN** a drag is in progress and the user presses Esc, then releases the button
- **THEN** no dialog opens and nothing changes

#### Scenario: Job completion mid-drag

- **WHEN** a drag is in progress and a running job's completion re-reads the panels
- **THEN** the drag is cleared and a subsequent release does nothing

### Requirement: Robust against listing changes

Dragged items SHALL be identified by name and source directory captured at drag start; streamed listing chunks or re-sorts during the drag SHALL NOT change them. At release the drop SHALL be cancelled if the source panel no longer shows the captured directory or the target row no longer resolves to a directory.

#### Scenario: Re-sort during drag

- **WHEN** the listing re-sorts while a drag is in progress and the user drops on the other panel
- **THEN** the dialog opens for exactly the items captured at drag start

#### Scenario: Source navigated away

- **WHEN** the source panel's directory changes during the drag
- **THEN** releasing does nothing
