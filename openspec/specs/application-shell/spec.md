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

The system SHALL acquire the alternate screen, raw mode, and (when enabled) mouse capture on startup and guarantee their release on every exit path — normal quit, error, and panic — via an RAII guard, so the user's terminal is never left in raw mode, on the alternate screen, or with mouse capture active after the process ends. When the TUI is suspended to run a shell command, an external editor, or the scrollback view, mouse capture SHALL be released before the alternate screen is left and re-acquired on resume.

#### Scenario: Terminal acquired on startup

- **WHEN** the application starts
- **THEN** it enters the alternate screen, enables raw mode, and enables mouse capture when configured

#### Scenario: Terminal restored on normal exit

- **WHEN** the application exits normally (for example via F10 quit)
- **THEN** mouse capture is disabled, raw mode is disabled, and the alternate screen is left before the process terminates

#### Scenario: Terminal restored on error exit

- **WHEN** the application exits because of an error after the terminal was acquired
- **THEN** the RAII guard still disables mouse capture and raw mode and leaves the alternate screen

#### Scenario: Suspended shell run gets a normal terminal

- **WHEN** the user runs a command from the command line
- **THEN** mouse capture is released before the child process runs and re-enabled after the TUI resumes

### Requirement: Panic hook restores the terminal before reporting

The system SHALL install a panic hook that disables mouse capture, leaves raw mode, and leaves the alternate screen BEFORE the panic report is printed, and that chains to the previously installed hook so the backtrace still surfaces, so that a panic while in raw mode never leaves the terminal unusable. The restoration and the chained report SHALL apply only to a panic on the UI thread, the thread that owns the terminal and runs the event loop.

A panic on a worker thread SHALL NOT restore the terminal and SHALL NOT write its report onto the terminal the UI still owns. Instead, the panic SHALL be contained to that worker and surfaced through the worker's normal result:

- a panicked directory listing SHALL end as a failed listing: the panel leaves its `Reading…` state and shows an error;
- a panicked file-operation job SHALL end as a cancelled job: the progress or prompt dialog closes, the affected panels re-read, and the end-of-job summary names the internal error;
- any other worker SHALL deliver its "no result" answer, so that no indicator stays pending.

The application SHALL keep running after a worker panic.

#### Scenario: Panic in raw mode restores the terminal first

- **WHEN** a panic occurs on the UI thread while the terminal is in raw mode on the alternate screen with mouse capture enabled
- **THEN** mouse capture is disabled, raw mode is disabled, and the alternate screen is left before any panic report is written

#### Scenario: Original hook still runs

- **WHEN** the panic hook completes its terminal restoration
- **THEN** it delegates to the previously installed hook so the panic message and backtrace are still reported

#### Scenario: A worker panic leaves the terminal alone

- **WHEN** a worker thread panics while the TUI is running in raw mode on the alternate screen
- **THEN** raw mode, the alternate screen, and mouse capture stay as they were, no panic text is written over the TUI, and the UI keeps responding to input

#### Scenario: A panicked listing ends as a failed listing

- **WHEN** the worker reading a panel's directory panics partway through
- **THEN** the panel stops showing `Reading… N`, shows an error for that directory, and can be re-read with Ctrl+R

#### Scenario: A panicked job ends and closes its dialog

- **WHEN** a file-operation job's worker panics while its progress dialog is shown
- **THEN** the progress dialog closes, the affected panels re-read, and an end-of-job summary names the internal error
- **AND** a new file operation can be started afterwards

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

### Requirement: Quit request keys and confirmation

The system SHALL open a quit-confirmation dialog — never exit directly — when the user requests quit via F10, Files → Quit, or Esc over the panels. Esc SHALL request quit whenever the panels own input (no pull-down menu and no modal dialog or overlay open), regardless of command-line content or an active quick filter or type-ahead mode; inside menus, dialogs, the viewer, and editor sub-prompts Esc SHALL retain its existing cancel/dismiss meaning. Ctrl+C SHALL NOT request quit in any context (over the panels it is the clipboard Files action — see `clipboard-export`; in the built-in editor it remains Copy). Within the quit-confirmation dialog, Esc SHALL cancel alongside the existing confirm/cancel keys; Ctrl+C SHALL have no effect there. Cancelling SHALL restore the prior context exactly — anything open stays open, and command-line text, quick-filter, and type-ahead state survive. Confirming while a file operation is running SHALL abort the job through its existing cancel semantics before the application exits.

#### Scenario: Esc over idle panels asks to quit

- **WHEN** the panels are shown with no menu, dialog, or overlay open and the user presses Esc
- **THEN** the quit-confirmation dialog opens
- **AND** confirming exits the application and cancelling returns to the panels unchanged

#### Scenario: Esc mid-composition asks to quit and cancel preserves the text

- **WHEN** the command-line buffer contains text and the user presses Esc, then cancels the dialog
- **THEN** the quit-confirmation dialog opened instead of the buffer being cleared
- **AND** after cancelling, the buffer still contains the typed text

#### Scenario: Esc under an active quick filter asks to quit

- **WHEN** a quick filter is active and the user presses Esc, then cancels the dialog
- **THEN** the quit-confirmation dialog opened instead of the filter being cleared
- **AND** after cancelling, the filter pattern and narrowed listing are unchanged

#### Scenario: Esc still cancels menus and dialogs

- **WHEN** a pull-down menu or a modal dialog is open and the user presses Esc
- **THEN** the menu or dialog closes as before and no quit-confirmation dialog opens

#### Scenario: Ctrl+C never asks to quit

- **WHEN** the user presses Ctrl+C over the panels, in the viewer, with a pull-down open, or with a modal dialog open
- **THEN** no quit-confirmation dialog opens (over the panels the clipboard Files action runs; elsewhere Ctrl+C neither quits nor triggers any unmodified-key action)

#### Scenario: Ctrl+C in the built-in editor still copies

- **WHEN** the built-in editor is open and the user presses Ctrl+C
- **THEN** the editor's Copy action runs and no quit-confirmation dialog opens

#### Scenario: Esc backs out of the dialog; Ctrl+C does nothing there

- **WHEN** the quit-confirmation dialog is open and the user presses Esc
- **THEN** the dialog closes and nothing else changes
- **WHEN** the quit-confirmation dialog is open and the user presses Ctrl+C
- **THEN** the dialog stays open and the application does not exit

#### Scenario: Quit confirmed during a running file operation aborts the job first

- **WHEN** a copy/move/delete job is running and the user requests quit via Esc, then confirms
- **THEN** the job is aborted through the same path as its cancel action before the application exits

### Requirement: Help flags print usage and exit before TUI startup

The system SHALL recognise `-h`, `-?`, and `--help` as the sole command-line argument, print a usage block to stdout, and exit with code 0 without acquiring the terminal, alternate screen, raw mode, or mouse capture. The usage block SHALL include: the binary name and version (sourced from `CARGO_PKG_VERSION`), a one-line description, a synopsis line, a flag table listing `-h` / `-?` / `--help`, `--theme <name>`, `--nomouse`, and `--nosplash`, and a line directing the user to press F1 inside the application for the full key reference.

#### Scenario: -h prints usage to stdout and exits 0

- **WHEN** the user runs `filecommand -h`
- **THEN** the usage block is written to stdout, the process exits with code 0, and the terminal is never placed into raw mode or the alternate screen

#### Scenario: -? prints usage to stdout and exits 0

- **WHEN** the user runs `filecommand -?`
- **THEN** the usage block is written to stdout and the process exits with code 0

#### Scenario: --help prints usage to stdout and exits 0

- **WHEN** the user runs `filecommand --help`
- **THEN** the usage block is written to stdout and the process exits with code 0

#### Scenario: Usage block includes version and flag table

- **WHEN** the usage block is printed
- **THEN** it includes the application name, the current version string, a synopsis, entries for `-h` / `-?` / `--help`, `--theme`, `--nomouse`, and `--nosplash`, and a note to press F1 for the full in-app key reference

#### Scenario: Help flag with additional arguments still prints usage

- **WHEN** the user runs `filecommand --help --theme nc-classic`
- **THEN** the usage block is printed and the process exits 0 without launching the TUI

#### Scenario: No help flag — TUI starts normally

- **WHEN** the application is invoked without `-h`, `-?`, or `--help`
- **THEN** no usage text is printed and the TUI starts as normal

### Requirement: Configuration profile directory

The system SHALL resolve one profile directory at startup and SHALL read and write `config.toml`, `history.json`, and `usermenu.toml` only there, never relative to the process working directory. Resolution order SHALL be: the `--config-dir <path>` launch flag; else, when a file named `portable` exists beside the executable, the executable's directory; else the platform default — `%APPDATA%\FileCommand` on Windows and `$XDG_CONFIG_HOME/filecommand` (falling back to `~/.config/filecommand`) elsewhere. The directory SHALL be created on first run. If it cannot be created or written, the session SHALL run on in-memory defaults, SHALL skip persistence, and SHALL raise the dismissable startup-warning dialog naming the path and the error. When the resolved profile holds no `config.toml` and the working directory contains any of the three files, the startup-warning dialog SHALL state where FileCommand now reads its files and that the old files may be moved; the system SHALL NOT copy, move, or delete them. The `--help` usage text SHALL list `--config-dir` and SHALL print the resolved default profile directory.

#### Scenario: Platform default is used when nothing overrides it
- **WHEN** FileCommand starts on Windows from `C:\Projects\app` with no `--config-dir` flag and no `portable` marker beside the executable
- **THEN** it reads and writes `config.toml`, `history.json`, and `usermenu.toml` under `%APPDATA%\FileCommand`, and no file is created in `C:\Projects\app`

#### Scenario: Explicit --config-dir override
- **WHEN** FileCommand is launched with `--config-dir D:\fc-test`
- **THEN** every configuration file is read from and written to `D:\fc-test`, and the portable marker and platform default are ignored

#### Scenario: Portable mode via marker file
- **WHEN** a file named `portable` exists in the executable's directory and no `--config-dir` flag is given
- **THEN** the executable's directory is the profile directory

#### Scenario: History and frecency are shared across launch directories
- **WHEN** the user visits directories in a session launched from `C:\a`, exits, and relaunches from `C:\b`
- **THEN** the Ctrl+J fuzzy-jump list and command history include the entries recorded in the first session

#### Scenario: Unwritable profile falls back without losing the session
- **WHEN** the profile directory cannot be created
- **THEN** the application starts with default settings, the startup-warning dialog names the path and error, and theme/split/history changes made during the session are not written to disk

#### Scenario: Legacy files in the working directory produce a notice, not a migration
- **WHEN** the profile directory has no `config.toml` and the working directory contains `config.toml` and `history.json`
- **THEN** the startup-warning dialog names those files, the working directory, and the profile directory, and both files are left exactly as they were

#### Scenario: Usage names the profile directory
- **WHEN** `filecommand --help` is run
- **THEN** the usage text lists `--config-dir <path>` and ends with the resolved default profile directory path

### Requirement: Optional diagnostic log

The system SHALL support an opt-in diagnostic log file, chosen at launch by the `--log <path>` flag (also accepted as `--log=<path>`) or by the `FILECOMMAND_LOG` environment variable. When both are given, the flag SHALL win.

When a log path is in effect, the system SHALL append one line per event to that file. Each line SHALL begin with a UTC timestamp, followed by a short category and a message naming the operation and the affected path where there is one. The file SHALL never be truncated. At least the following SHALL be logged, most of which the application otherwise handles without telling the user:

- failures to write `history.json` or `config.toml`;
- results that could not be delivered between a worker thread and the event loop;
- answers that could not be delivered to a running file operation;
- metadata that could not be restored on a copied file;
- every startup warning, including any that a later warning replaced in the startup-warning dialog.

When neither the flag nor the variable is set, the system SHALL NOT create or write any log file.

If the log file cannot be opened for appending, or the flag is given without a path, the session SHALL start without logging and SHALL raise the dismissable startup-warning dialog naming the path and the reason. A failure to write a log line SHALL never interrupt the session.

The `--help` usage block SHALL list `--log <path>` and SHALL mention `FILECOMMAND_LOG`. The log SHALL be implemented without adding a dependency, and `filecommand-core` SHALL NOT write to it directly.

#### Scenario: No log unless asked for

- **WHEN** FileCommand runs without `--log` and with `FILECOMMAND_LOG` unset, and a `history.json` write fails during the session
- **THEN** no log file is created anywhere, and the session continues as before

#### Scenario: A failed history write is logged with a timestamp

- **WHEN** FileCommand is launched with `--log C:\temp\fc.log` and a `history.json` write fails because the profile directory became read-only
- **THEN** `C:\temp\fc.log` gains a line beginning with a UTC timestamp that names the history write and the path of `history.json`, together with the operating-system error
- **AND** no dialog or status-line message is shown for it

#### Scenario: The environment variable enables logging

- **WHEN** `FILECOMMAND_LOG` is set to `D:\logs\fc.log` and FileCommand is launched without `--log`
- **THEN** diagnostic lines are appended to `D:\logs\fc.log`

#### Scenario: The flag wins over the environment variable

- **WHEN** `FILECOMMAND_LOG` is `D:\logs\env.log` and FileCommand is launched with `--log=D:\logs\flag.log`
- **THEN** lines are appended to `D:\logs\flag.log` and `D:\logs\env.log` is not created or written

#### Scenario: Existing log content is kept

- **WHEN** the log file already holds lines from an earlier session
- **THEN** the new session's lines are appended after them

#### Scenario: Unwritable log path warns and continues

- **WHEN** FileCommand is launched with `--log` naming a file in a directory that does not exist
- **THEN** the TUI starts normally, the startup-warning dialog names that path and the reason it could not be opened, and nothing is logged for the session

#### Scenario: Superseded startup warnings are still recorded

- **WHEN** logging is enabled and two startup warnings occur, such as an unknown `--theme` and a malformed `usermenu.toml`, so the dialog shows only the last one
- **THEN** the log holds a line for each of the two warnings

#### Scenario: Usage lists the log options

- **WHEN** the user runs `filecommand --help`
- **THEN** the usage block lists `--log <path>` and mentions the `FILECOMMAND_LOG` environment variable

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

### Requirement: Ctrl+C during a suspended child run stops only the child

While FileCommand is suspended and waiting for a child process it started, a Ctrl+C or Ctrl+Break typed at the console SHALL reach the child as usual and SHALL NOT terminate FileCommand. This covers a command run from the file-action menu's Run entry, a command from the F2 user menu, and the external editor launched by F4.

After the child exits, FileCommand SHALL show its usual press-any-key prompt and then resume the TUI with its tabs, panels, command history, and session state intact. The child SHALL NOT inherit an instruction to ignore Ctrl+C from FileCommand, so a console program that stops on Ctrl+C still stops.

Outside these suspended child runs, Ctrl+C handling SHALL be unchanged, as SHALL any console close, logoff, or shutdown event, which still ends FileCommand.

#### Scenario: Ctrl+C stops a user-menu command and FileCommand survives

- **WHEN** the user runs a user-menu entry `ping -t 127.0.0.1` and presses Ctrl+C while it runs
- **THEN** `ping` stops and FileCommand shows its press-any-key prompt
- **AND** after a key press the TUI resumes with the same tabs, directories, and cursor positions as before the command ran

#### Scenario: Ctrl+C during a Run-entry command

- **WHEN** the user activates Run on `server.exe`, a console program that stops on Ctrl+C, and presses Ctrl+C
- **THEN** `server.exe` stops, and FileCommand is still running and resumes after the prompt

#### Scenario: Ctrl+C inside the external editor does not end FileCommand

- **WHEN** the configured external editor is running in the suspended terminal and the user presses Ctrl+C
- **THEN** FileCommand does not exit, whether or not the editor itself treats Ctrl+C as a key or exits on it, and the panel re-read and resume happen as usual once the editor exits

#### Scenario: The child is not made to ignore Ctrl+C

- **WHEN** a child started through any of these paths installs no handler of its own and the user presses Ctrl+C
- **THEN** the child terminates as it would when run directly from a shell

### Requirement: Quit waits for job cancellation cleanup

When quit is confirmed while a file-operation job is running, the system SHALL do the following before the process exits:

- request the job's cancellation;
- treat any overwrite-conflict or error-recovery prompt the job is waiting on as an abort, without requiring an answer;
- wait for the job to finish its cancellation cleanup, including removing a partially written destination file, for up to about 2 seconds.

If the job has not finished within that bound, the application SHALL exit anyway. The wait SHALL apply to any job still running when quit is confirmed, whichever screen or dialog is showing at the time. When no job is running, quit SHALL exit without delay.

#### Scenario: Quit during a large copy leaves no partial file

- **WHEN** a copy of a 2 GB file is in progress and the user requests quit and confirms
- **THEN** the application exits only after the partially written destination file has been removed, and the destination holds no truncated copy of that file

#### Scenario: Quit while a conflict prompt is waiting

- **WHEN** a copy job is waiting on the overwrite-conflict dialog and the user requests quit and confirms
- **THEN** the job stops without overwriting the conflicting target and without processing any further items, and the application exits after the job has ended

#### Scenario: The wait is bounded

- **WHEN** quit is confirmed while the job is blocked in a write to a share that has stopped responding
- **THEN** the application exits after about 2 seconds rather than waiting for the write to return

#### Scenario: No job means no wait

- **WHEN** no file operation is running and the user confirms quit
- **THEN** the application exits immediately

