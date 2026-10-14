## ADDED Requirements

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

## MODIFIED Requirements

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
