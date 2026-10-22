## ADDED Requirements

### Requirement: The reducer performs no filesystem I/O

`core::update` SHALL NOT perform filesystem I/O of any kind, directly or through any helper it calls. That includes metadata or existence queries, directory reads, and file opens. When handling a command depends on the current state of the filesystem, `core::update` SHALL:

- answer from the state it already holds where that state suffices, such as an entry already listed in a panel;
- otherwise return an effect requesting the check, tagged with a fresh request id;
- complete the command only when the reply command carrying that id is applied.

A reply whose id is no longer the outstanding request SHALL be discarded without changing state. This makes the "Pure core update function" rule checkable by a unit test that runs with no filesystem at all.

#### Scenario: A command-line target outside the listing produces a probe request, not a disk read

- **WHEN** a unit test applies `cd sub` to `core::update` with the active panel at `C:\NORTON` and `sub` not among its listed entries
- **THEN** the returned effects contain a path-probe request for `C:\NORTON\sub` tagged with a request id, and the panel's `cwd` is still `C:\NORTON`
- **AND** `core::update` made no filesystem call, so the result is the same whether or not `C:\NORTON\sub` exists on the machine running the test

#### Scenario: The probe reply completes the command

- **WHEN** the path-probe reply for that request id arrives reporting a directory
- **THEN** applying it through `core::update` navigates the panel to `C:\NORTON\sub` exactly as a synchronous check would have

#### Scenario: A superseded probe reply is discarded

- **WHEN** a second `cd` is entered before the first probe's reply arrives, and the first reply then arrives
- **THEN** the first reply changes nothing: no navigation, no error message, and no dialog

### Requirement: Slow storage never blocks the event loop

These operations SHALL run on worker threads, never on the event-loop thread, so that a slow, removable, or unreachable volume cannot freeze input or redraw:

- the command-line target check;
- opening a file in the F3 viewer;
- opening a file in the built-in editor;
- saving from the built-in editor;
- launching a file's associated application;
- writing the applied theme and the panel split to `config.toml`.

While an editor or viewer open is outstanding, the status line of the panel it was requested from SHALL show `Opening <name>…`, and the panels SHALL keep accepting input. A newer F3 or F4 request SHALL supersede an outstanding open. An open reply that arrives after the user has left the panels, or after a menu, dialog, or overlay has opened, SHALL be discarded, and a viewer reply's file handle SHALL be released.

While a save is outstanding, the editor SHALL show a `Saving…` notice on the row directly above its F-key bar, leaving the header and F-key bar unchanged, and SHALL NOT accept edits, caret moves, or further save or quit commands until the save completes or fails. The screen SHALL keep redrawing meanwhile.

Configuration writes SHALL be performed one at a time, in the order they were requested, by a single background writer, so that two rewrites of `config.toml` never interleave. A newer value for a setting SHALL replace an older value for the same setting that has not been written yet, so the value on disk after the writes complete is always the last one requested. Moving a write to the background writer SHALL NOT change how it is written: it SHALL remain the atomic write that theme-selection "Applied theme persists to configuration" and panel-split "Split persistence to configuration" require, made durable as configuration-files "Persisted configuration writes are durable" specifies where that requirement exists, and a failed write SHALL NOT interrupt the session. When the application exits, by quit or by any other exit path, the system SHALL first complete every configuration write still outstanding, waiting at most about 2 seconds. History persistence (`history.json`) is outside this rule.

On Windows, the worker that launches the associated application SHALL initialize COM in a single-threaded apartment (`COINIT_APARTMENTTHREADED`) before calling the shell launch API and SHALL uninitialize it afterwards. A launch failure SHALL be reported on the status line of the panel the Open action was invoked on.

#### Scenario: F4 on a slow share keeps the panels live

- **WHEN** the user presses F4 on `notes.txt` in a panel showing `\\nas\share` and reading the file takes several seconds
- **THEN** the panels keep redrawing and moving the cursor in response to keys while the panel's status line shows `Opening notes.txt…`
- **AND** the built-in editor opens on `notes.txt` once the read completes

#### Scenario: F3 on a file whose server stopped responding does not freeze the UI

- **WHEN** the user presses F3 on a file on a share whose server has stopped responding
- **THEN** the UI keeps redrawing and accepting input while the open is outstanding
- **AND** when the open finally fails, the panel's status line shows the failure and the viewer does not open

#### Scenario: Saving to a slow share shows progress and accepts no edits

- **WHEN** the user presses F2 in the built-in editor on a file whose write takes several seconds
- **THEN** the editor shows `Saving…` above its F-key bar, the screen keeps redrawing, and typed characters are not inserted into the buffer
- **AND** when the write completes, `Saving…` disappears, the modified indicator clears, and typing edits the buffer again

#### Scenario: A newer F3 supersedes an outstanding open

- **WHEN** the user presses F3 on `slow.log`, moves to `fast.txt`, and presses F3 again before `slow.log` has opened
- **THEN** the viewer opens `fast.txt`, and the late answer for `slow.log` is discarded without replacing it

#### Scenario: A late open does not pull the user out of a menu

- **WHEN** an F4 open is outstanding and the user opens the F9 menu before it completes
- **THEN** the menu stays open, the editor does not open when the read completes, and the late result is discarded

#### Scenario: Launching a slow handler leaves the panels interactive

- **WHEN** the user activates Open on a document whose associated application takes several seconds to start
- **THEN** the panels keep redrawing and responding to keys during the launch
- **AND** if the launch fails, the failure is shown on the status line of the panel the Open action was invoked on, even if the user has switched panels since

#### Scenario: COM is initialized on the launch worker

- **WHEN** Open launches a file on Windows
- **THEN** the shell launch API is called on a worker thread that initialized COM in a single-threaded apartment beforehand, and that thread uninitializes COM after the call returns

#### Scenario: Applying a theme on a slow profile directory keeps the UI live

- **WHEN** the profile directory is on a network share that takes several seconds to answer and the user applies a theme from the theme picker
- **THEN** the new theme is shown and the UI keeps redrawing and accepting keys while `config.toml` is being rewritten in the background

#### Scenario: Rapid split adjustments persist the final value

- **WHEN** the split is at 50/50 on a 100-column terminal and the user presses Ctrl+Right eight times in quick succession, ending at a 66% split
- **THEN** the input is never held up by the writes, and once the writer has caught up `config.toml` contains `panel_split = 66`

#### Scenario: A theme and a split change made together are both saved

- **WHEN** the user applies a theme and, before that write finishes, adjusts the split
- **THEN** after both writes complete `config.toml` contains both the new `theme` and the new `panel_split`

#### Scenario: A setting changed just before quit is saved

- **WHEN** the user adjusts the split and immediately quits and confirms, on a responsive disk
- **THEN** the process exits only after `config.toml` holds the new `panel_split` value

#### Scenario: An unresponsive profile directory cannot hold quit open

- **WHEN** a configuration write is blocked on a profile directory that has stopped answering and the user confirms quit
- **THEN** the application exits after about 2 seconds, and `config.toml` is left with either its complete previous content or its complete new content
