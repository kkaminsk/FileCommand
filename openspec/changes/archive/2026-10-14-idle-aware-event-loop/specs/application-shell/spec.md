## ADDED Requirements

### Requirement: Idle event loop does not redraw

The system SHALL redraw the screen only when something visible may have changed: when input or a worker result has been applied, or when a time-driven change becomes due — a frame of a running animation, the expiry of clipboard feedback, the end of the splash hold, or a minute boundary of the header clock. While none of these occurs the system SHALL NOT redraw, and SHALL NOT wake more often than about every 33 ms, and then only while worker threads that can still deliver results are running. Time-driven behavior SHALL keep its timing: animations SHALL still advance every frame from the reducer clock until they finish, clipboard feedback SHALL still expire on time without input, and the splash SHALL still end after its hold. Before input is applied after an idle period, the system SHALL bring the reducer clock up to date, so that a surface opened after idling animates from the moment it opened and two clicks separated by idle time are not treated as a double-click. A pending redraw SHALL NOT be lost when the input that follows it produces no command.

#### Scenario: Idle screen is not redrawn

- **WHEN** the panels are shown with no input arriving, no worker thread running, no animation in progress, no clipboard feedback shown, and no minute boundary passing for 30 seconds
- **THEN** no frame is drawn during those 30 seconds

#### Scenario: Header clock still advances while idle

- **WHEN** the application is idle and the wall clock passes a minute boundary
- **THEN** the screen is redrawn once at that boundary and the header clock shows the new minute

#### Scenario: Animations still run after idling

- **WHEN** the application has been idle for a minute with animations enabled and the user presses F9
- **THEN** the menu bar sweeps and the `Left` pull-down unfolds over their normal durations with intermediate frames, rather than appearing complete on the first frame

#### Scenario: Clipboard feedback expires without input

- **WHEN** clipboard feedback is shown and the user presses no key
- **THEN** the feedback disappears and the mini-status reverts when its expiry time is reached

#### Scenario: Worker results still appear promptly

- **WHEN** a directory listing is being read while the user presses no key
- **THEN** each batch of entries is shown within about 33 ms of arriving, and once the listing completes and no worker remains the screen stops redrawing

#### Scenario: Clicks separated by idle time are not a double-click

- **WHEN** the user clicks an entry, the application sits idle for 10 seconds, and the user clicks the same entry again
- **THEN** the second click is handled as a single click, not as a double-click

#### Scenario: Redraw survives an unmapped key

- **WHEN** a worker result is applied and, in the same loop iteration, the user presses a key that maps to no command
- **THEN** the screen is still redrawn to show the worker result

### Requirement: Bursts of key input render once

When several input events are already queued, the system SHALL apply them in order — mapping each key against the state produced by the previous one — and SHALL draw once after the burst instead of once per key. A burst SHALL end after 64 events, after about 8 ms of processing, when no further input is queued, or when a mouse event is reached; the system SHALL then draw before handling further input, so the screen never falls far behind. Applying a burst SHALL produce the same final state as applying the same keys one at a time.

#### Scenario: Holding PgDn renders one frame per burst

- **WHEN** five PgDn key events are queued at once in a 100,000-entry directory
- **THEN** all five page movements are applied and the screen is drawn once, showing the final position

#### Scenario: Routing follows the state each key produces

- **WHEN** the queued keys are F7 followed by the characters `new` and Enter while the panels are focused
- **THEN** F7 opens the Make-directory dialog, `new` is typed into its field, and Enter confirms it, exactly as if the keys had been pressed slowly

#### Scenario: A paste delivered as key events renders once per burst

- **WHEN** the terminal delivers a 60-character pasted path as 60 key events while the command line has focus
- **THEN** all 60 characters are inserted in order and the screen is drawn at most once for every 64 of them
