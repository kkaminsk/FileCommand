# application-shell Specification

## Purpose
TBD - created by archiving change m1-shell. Update Purpose after archive.
## Requirements
### Requirement: Cargo workspace and core/tui crate boundary

The system SHALL be structured as a Cargo workspace with two crates — `filecommand-core` (a platform-agnostic library) and `filecommand-tui` (the binary that owns the terminal, event loop, and rendering) — and the `filecommand-core` crate MUST NOT depend on `ratatui` or `crossterm`, so that all application state logic remains unit-testable without a terminal.

#### Scenario: Core crate carries no terminal dependencies

- **WHEN** the dependency graph of `filecommand-core` is inspected
- **THEN** neither `ratatui` nor `crossterm` (directly or transitively as a normal dependency) appears in it
- **AND** the crate compiles and its unit tests run without acquiring a terminal

#### Scenario: Dependency direction is one-way

- **WHEN** the workspace is built
- **THEN** `filecommand-tui` depends on `filecommand-core`
- **AND** `filecommand-core` has no dependency on `filecommand-tui`

#### Scenario: Workspace builds both crates

- **WHEN** the workspace is built from its root manifest
- **THEN** both the `filecommand-core` library and the `filecommand-tui` binary compile successfully

### Requirement: Pure core update function

The system SHALL mutate application state exclusively through a pure function `core::update(state, command) -> (state, effects)` that performs no I/O, so that the same command sequence always yields the same next state and the same requested effects, and the tui crate — not `core` — executes those effects.

#### Scenario: Update is deterministic and side-effect free

- **WHEN** `core::update` is called twice with equal starting state and equal command
- **THEN** both calls return equal next state and equal effect lists
- **AND** neither call performs terminal, filesystem, thread, or timing side effects itself

#### Scenario: Effects are returned, not executed, by core

- **WHEN** a command whose handling requires a directory read is applied to `core::update`
- **THEN** the returned state reflects the request and the returned effect list contains an intent to start that listing
- **AND** `core::update` itself does not spawn a thread or read the directory

#### Scenario: Worker results re-enter through the same update path

- **WHEN** a worker-produced event (such as a directory-listing chunk) is delivered
- **THEN** it is converted into a command and applied via `core::update` on the same path as key-derived commands

### Requirement: Single-threaded UI event loop with worker threads

The system SHALL run one thread that owns the terminal and drives the event loop, draining a single queue that merges crossterm input events and worker events into commands applied through `core::update` and then redrawing, while offloading directory reads to worker threads that stream results back over a channel. The first painted frame MUST NOT wait on any directory I/O.

#### Scenario: First paint does not block on listing

- **WHEN** the application starts and the initial directory listing has not yet completed
- **THEN** the first frame is painted without waiting for that listing to finish

#### Scenario: Directory reads run off the UI thread

- **WHEN** a directory listing is requested
- **THEN** the read is performed on a worker thread and its results are sent back to the UI thread over a channel
- **AND** the UI thread remains responsive to input while the listing streams in

#### Scenario: Input and worker events share one queue

- **WHEN** both a pending input event and a pending worker event exist in the same loop iteration
- **THEN** each is turned into a command and applied through `core::update`, and the screen is redrawn from the resulting state

### Requirement: Terminal ownership and restoration on every exit

The system SHALL acquire the alternate screen and raw mode on startup and guarantee their release on every exit path — normal quit, error, and panic — via an RAII guard, so the user's terminal is never left in raw mode or on the alternate screen after the process ends.

#### Scenario: Terminal acquired on startup

- **WHEN** the application starts
- **THEN** it enters the alternate screen and enables raw mode

#### Scenario: Terminal restored on normal exit

- **WHEN** the application exits normally (for example via F10 quit)
- **THEN** raw mode is disabled and the alternate screen is left before the process terminates

#### Scenario: Terminal restored on error exit

- **WHEN** the application exits because of an error after the terminal was acquired
- **THEN** the RAII guard still disables raw mode and leaves the alternate screen

### Requirement: Panic hook restores the terminal before reporting

The system SHALL install a panic hook that leaves raw mode and the alternate screen BEFORE the panic report is printed, and that chains to the previously installed hook so the backtrace still surfaces, so that a panic while in raw mode never leaves the terminal unusable.

#### Scenario: Panic in raw mode restores the terminal first

- **WHEN** a panic occurs while the terminal is in raw mode on the alternate screen
- **THEN** raw mode is disabled and the alternate screen is left before any panic report is written

#### Scenario: Original hook still runs

- **WHEN** the panic hook completes its terminal restoration
- **THEN** it delegates to the previously installed hook so the panic message and backtrace are still reported

### Requirement: Resize handling with 80x24 minimum and placeholder

The system SHALL reflow the UI on terminal resize events, laying out the interface only at or above the 80x24 minimum, and MUST draw a `screen.placeholder` "terminal too small" message instead whenever the terminal is below that minimum, using a single size check that governs both normal and splash states.

#### Scenario: Reflow at or above minimum

- **WHEN** the terminal is resized to a size at or above 80x24
- **THEN** the UI reflows and lays out its regions to the new dimensions

#### Scenario: Placeholder below minimum

- **WHEN** the terminal is below 80 columns or below 24 rows
- **THEN** the "terminal too small" placeholder message is drawn instead of the normal layout

#### Scenario: Shrinking below minimum during splash

- **WHEN** the terminal shrinks below 80x24 while the startup splash is showing
- **THEN** the placeholder replaces the splash, and the splash does not return when the terminal is enlarged again

#### Scenario: Recovery when resized back up

- **WHEN** the terminal is enlarged from below the minimum back to at or above 80x24
- **THEN** the normal layout is drawn again in place of the placeholder

