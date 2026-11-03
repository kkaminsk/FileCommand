## ADDED Requirements

### Requirement: Panels on/off reveals terminal scrollback

When the user presses Ctrl+O, the system SHALL leave the alternate screen to expose the host terminal's scrollback containing prior command output, and any subsequent key press SHALL return to the alternate screen and redraw. The system SHALL NOT maintain its own command-output buffer; the visible output history is whatever the host terminal retains.

#### Scenario: Ctrl+O leaves the alternate screen
- **WHEN** the user presses Ctrl+O
- **THEN** the app leaves the alternate screen so the terminal's scrollback (including prior command output) is visible

#### Scenario: Any key returns to panels
- **WHEN** the terminal scrollback is showing after Ctrl+O and the user presses any key
- **THEN** the app re-enters the alternate screen and redraws the panels

### Requirement: Shell commands run only from the Run entry and the user menu

The system SHALL NOT run typed text through a shell. The suspended-TUI shell-spawn mechanism SHALL be reachable only from the file-action menu's Run entry and the F2 user menu, each via its own trigger. Enter on an executable target (PATHEXT match or `.lnk`) SHALL NOT spawn the target directly; it SHALL open the file-action menu for that entry, whose Run entry uses this spawn path.

#### Scenario: Enter on an executable opens the menu instead of spawning
- **WHEN** the user presses Enter with the cursor on `setup.exe`
- **THEN** no child process spawns and the file-action menu opens for `setup.exe`
- **AND** activating the menu's Run entry spawns `setup.exe` via the suspended-TUI path

#### Scenario: Typed text never reaches a shell
- **WHEN** a path box has focus, reads `C:\NORTON\dir` and the user presses Enter
- **THEN** no shell process is spawned; the text is treated as a path per `path-box`

### Requirement: A suspended shell run restores the terminal

Running a command through the suspended-TUI mechanism SHALL leave raw mode and the alternate screen, spawn the child inheriting stdio in the panel directory, wait for it to exit, prompt the user to press a key, then re-enter the alternate screen and raw mode and redraw. Terminal restore SHALL be idempotent so a failing or panicking child cannot leave the terminal in raw mode or the alternate screen.

#### Scenario: Terminal restored after a failing child
- **WHEN** a child spawned via the Run entry or F2 user menu exits with an error or the spawn fails
- **THEN** the TUI is restored to the alternate screen and raw mode exactly once, and the app does not crash

### Requirement: Configurable shell with documented latency tradeoff

The system SHALL construct the shell invocation from a configurable shell setting, defaulting on Windows to `cmd.exe /C` for minimal spawn latency. The `config.toml` `shell =` key SHALL select an alternate shell such as PowerShell or `pwsh`, and the configuration SHALL document that PowerShell adds roughly 200 ms or more of spawn latency per command. The shell invocation SHALL be constructed as `shell + args + user text` in `filecommand-core` so it is unit-testable without a terminal.

#### Scenario: Default shell is cmd.exe on Windows
- **WHEN** no `shell =` value is configured on Windows and a user-menu entry runs `dir`
- **THEN** the invocation is built from `cmd.exe /C` with the entry's text appended

#### Scenario: Configured PowerShell shell is used
- **WHEN** `config.toml` sets `shell = "powershell"` and the user runs a user-menu entry
- **THEN** the invocation is built from the configured PowerShell executable rather than `cmd.exe`

#### Scenario: Command construction is terminal-independent
- **WHEN** a unit test constructs a command line for the text `dir` with the default shell
- **THEN** the constructed invocation and working directory are produced by `filecommand-core` without requiring a terminal or an actual spawn

## MODIFIED Requirements

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

- **WHEN** the user runs a command from the file-action menu's Run entry or the F2 user menu
- **THEN** mouse capture is released before the child process runs and re-enabled after the TUI resumes

### Requirement: Resize handling with 60x16 hard floor and placeholder

The system SHALL reflow the UI on terminal resize events, laying out the interface at any size at or above the 60x16 hard floor, and MUST draw a `screen.placeholder` "terminal too small" message instead whenever the terminal is below that floor, using a single size check that governs both normal and splash states. The placeholder message SHALL name the floor: "resize to at least 60x16". Between the floor and the 80x24 nominal size the interface renders in the degraded forms defined by the `responsive-layout` capability; at or above 80x24 it renders at full fidelity.

The placeholder SHALL be drawn over the active screen without discarding it. While the terminal is below the floor, the following SHALL persist unchanged and SHALL reappear when the terminal is enlarged to at or above the floor:

- the panels and their tabs, cursor, selection, quick filter, and a focused path box with its text;
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
- **THEN** the panels, path boxes, and key bar render in their degraded forms and the placeholder is not shown

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

The system SHALL open a quit-confirmation dialog — never exit directly — when the user requests quit via F10, Files → Quit, or Esc over the panels. Esc SHALL request quit whenever the panels own input (no pull-down menu and no modal dialog or overlay open), regardless of an active quick filter or type-ahead mode — while a path box has focus, Esc instead cancels the box (path-box "Esc cancels the path box"); inside menus, dialogs, the viewer, and editor sub-prompts Esc SHALL retain its existing cancel/dismiss meaning. Ctrl+C SHALL NOT request quit in any context (over the panels it is the clipboard Files action — see `clipboard-export`; in the built-in editor it remains Copy). Within the quit-confirmation dialog, Esc SHALL cancel alongside the existing confirm/cancel keys; Ctrl+C SHALL have no effect there. Cancelling SHALL restore the prior context exactly — anything open stays open, and quick-filter and type-ahead state survive. Confirming while a file operation is running SHALL abort the job through its existing cancel semantics before the application exits.

#### Scenario: Esc over idle panels asks to quit

- **WHEN** the panels are shown with no menu, dialog, or overlay open and the user presses Esc
- **THEN** the quit-confirmation dialog opens
- **AND** confirming exits the application and cancelling returns to the panels unchanged

#### Scenario: Esc mid-composition asks to quit and cancel preserves the text

- **WHEN** a path box has focus with edited text and the user presses Esc
- **THEN** the box closes and resets to the panel's directory and no quit-confirmation dialog opens
- **AND** a second Esc, now over the panels, opens the quit-confirmation dialog

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

### Requirement: Bursts of key input render once

When several input events are already queued, the system SHALL apply them in order — mapping each key against the state produced by the previous one — and SHALL draw once after the burst instead of once per key. A burst SHALL end after 64 events, after about 8 ms of processing, when no further input is queued, or when a mouse event is reached; the system SHALL then draw before handling further input, so the screen never falls far behind. Applying a burst SHALL produce the same final state as applying the same keys one at a time.

#### Scenario: Holding PgDn renders one frame per burst

- **WHEN** five PgDn key events are queued at once in a 100,000-entry directory
- **THEN** all five page movements are applied and the screen is drawn once, showing the final position

#### Scenario: Routing follows the state each key produces

- **WHEN** the queued keys are F7 followed by the characters `new` and Enter while the panels are focused
- **THEN** F7 opens the Make-directory dialog, `new` is typed into its field, and Enter confirms it, exactly as if the keys had been pressed slowly

#### Scenario: A paste delivered as key events renders once per burst

- **WHEN** the terminal delivers a 60-character pasted path as 60 key events while a path box has focus
- **THEN** all 60 characters are inserted in order and the screen is drawn at most once for every 64 of them

### Requirement: The reducer performs no filesystem I/O

`core::update` SHALL NOT perform filesystem I/O of any kind, directly or through any helper it calls. That includes metadata or existence queries, directory reads, and file opens. When handling a command depends on the current state of the filesystem, `core::update` SHALL:

- answer from the state it already holds where that state suffices, such as an entry already listed in a panel;
- otherwise return an effect requesting the check, tagged with a fresh request id;
- complete the command only when the reply command carrying that id is applied.

A reply whose id is no longer the outstanding request SHALL be discarded without changing state. This makes the "Pure core update function" rule checkable by a unit test that runs with no filesystem at all.

#### Scenario: A command-line target outside the listing produces a probe request, not a disk read

- **WHEN** a unit test applies Enter in a path box reading `C:\NORTON\sub\x.txt` to `core::update` with `C:\NORTON\sub` the current directory of neither panel
- **THEN** the returned effects contain a path-probe request for `C:\NORTON\sub\x.txt` tagged with a request id, and the panel's `cwd` is unchanged
- **AND** `core::update` made no filesystem call, so the result is the same whether or not `C:\NORTON\sub\x.txt` exists on the machine running the test

#### Scenario: The probe reply completes the command

- **WHEN** the path-probe reply for that request id arrives reporting that the entry exists
- **THEN** applying it through `core::update` navigates the panel to `C:\NORTON\sub` and lands the cursor on `x.txt` exactly as a synchronous check would have

#### Scenario: A superseded probe reply is discarded

- **WHEN** a second Enter is applied before the first probe's reply arrives, and the first reply then arrives
- **THEN** the first reply changes nothing: no navigation, no error message, and no dialog

#### Scenario: A completion read is a request, not a disk read

- **WHEN** a unit test applies Tab in a path box reading `D:\Proj\re` with `D:\Proj` the current directory of neither panel
- **THEN** the returned effects contain a completion-read request for `D:\Proj` tagged with a request id, and the box text is unchanged
- **AND** `core::update` made no filesystem call

### Requirement: Slow storage never blocks the event loop

These operations SHALL run on worker threads, never on the event-loop thread, so that a slow, removable, or unreachable volume cannot freeze input or redraw:

- the path-box target check and the path-box completion read;
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

### Requirement: Modal overlays are mutually exclusive by construction

The system SHALL hold the state of the modal overlays in a single overlay slot in `filecommand-core`'s `State`, so that at most one of them is open at any time. The modal overlays are the F9 menu, the drive-select dialog, fuzzy jump, find file, the F2 user menu, the theme picker, the file-action menu, the Help window (with its nested About dialog), and the startup warning. Each overlay's handler SHALL receive that overlay's state directly from the slot, so no handler depends on a separately checked gate and none can run while its overlay is closed.

The quit-confirmation dialog is not part of the slot. It SHALL remain a separate layer above every other context, as "Quit request keys and confirmation" requires. The file-operation dialogs, the viewer, and the editor SHALL remain phases.

Which layer owns input SHALL be defined once, by a single precedence order in `filecommand-core`, and the reducer's command routing, the TUI's key routing, its mouse routing, and its rendering and hit-map construction SHALL all follow it. The order, highest first:
1. the quit confirmation;
2. a file-operation dialog;
3. the viewer;
4. the editor;
5. the open overlay;
6. the panels or the startup splash.

A layer SHALL handle only the commands it claims; any other command, including worker replies, SHALL pass to the layers below it as it does today. While the terminal is below the 60x16 floor, input routing SHALL additionally follow "Resize handling with 60x16 hard floor and placeholder".

This structure SHALL NOT change which overlay is shown, which keys or clicks it accepts, or how it renders, in any state the application can reach.

#### Scenario: A menu item opens its dialog in place of the menu

- **WHEN** the F9 menu is open and the user activates Commands → Find file
- **THEN** the menu closes and the find-file dialog opens
- **AND** only the find-file dialog is shown, and it receives the next key

#### Scenario: The user menu's Themes entry replaces the user menu

- **WHEN** the F2 user menu is open with its built-in Themes entry highlighted and the user presses Enter
- **THEN** the user menu closes and the theme picker opens in its place, with the active theme highlighted

#### Scenario: Another overlay's shortcut is ignored while an overlay is open

- **WHEN** the fuzzy-jump dialog is open and the user presses F9 or Alt+F1
- **THEN** neither the menu nor the drive-select dialog opens
- **AND** the fuzzy-jump dialog stays open with its pattern and highlight unchanged

#### Scenario: Unbound keys never reach the panels or the command line

- **WHEN** the F2 user menu is open and the user types a printable character that the user menu does not bind
- **THEN** the key is ignored, so the path boxes and the panel cursor are unchanged

#### Scenario: Late results of a dismissed search touch no other overlay

- **WHEN** the user dismisses the find-file dialog with Esc while its search is still running, then opens the Help window
- **AND** further matches from the dismissed search arrive
- **THEN** the results are discarded, the Help window is unchanged, and no find-file dialog reappears

#### Scenario: An overlay command with its overlay closed does nothing

- **WHEN** a command that only an overlay binds (for example confirming fuzzy jump) reaches the reducer while that overlay is not open
- **THEN** the state is unchanged, no effect is returned, and nothing panics

#### Scenario: The quit dialog stacks above an open overlay

- **WHEN** the quit-confirmation dialog is open above an open F9 pull-down and the user cancels it
- **THEN** the pull-down is still open exactly as before, and it receives the next key

#### Scenario: Worker replies pass beneath the quit dialog

- **WHEN** a copy job is running, the quit-confirmation dialog is open above its progress dialog, and the job reports progress
- **THEN** the progress dialog underneath shows the new progress and the quit-confirmation dialog stays open

#### Scenario: One precedence order everywhere

- **WHEN** the routing of a key, the routing of a mouse event, and the drawing and hit map of a frame are each computed for the same state
- **THEN** all of them treat the same layer as the one that owns input, the layer defined by the single precedence order
