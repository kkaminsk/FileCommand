## MODIFIED Requirements

### Requirement: Idle event loop does not redraw

The system SHALL redraw the screen only when something visible may have changed: when input or a worker result that changed state has been applied, or when a time-driven change becomes due — a frame of a running animation, the expiry of clipboard feedback, the end of the splash hold, or a minute boundary of the header clock. The automatic directory check of `auto-refresh` is a time-driven wake that SHALL fall due only at its configured interval, and a check result that finds no change SHALL NOT cause a redraw. While none of these occurs the system SHALL NOT redraw, and SHALL NOT wake more often than about every 33 ms, and then only while worker threads that can still deliver results are running. Time-driven behavior SHALL keep its timing: animations SHALL still advance every frame from the reducer clock until they finish, clipboard feedback SHALL still expire on time without input, and the splash SHALL still end after its hold. Before input is applied after an idle period, the system SHALL bring the reducer clock up to date, so that a surface opened after idling animates from the moment it opened and two clicks separated by idle time are not treated as a double-click. A pending redraw SHALL NOT be lost when the input that follows it produces no command.

#### Scenario: Idle screen is not redrawn

- **WHEN** the panels are shown with no input arriving, no worker thread running, no animation in progress, no clipboard feedback shown, no automatic check falling due, and no minute boundary passing for 30 seconds
- **THEN** no frame is drawn during those 30 seconds

#### Scenario: An unchanged directory check draws nothing

- **WHEN** the panels sit idle across an automatic directory check whose directories have not changed
- **THEN** the only wake-ups are the check's issue and its result, and no frame is drawn for either

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
