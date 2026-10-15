# fuzzy-jump Specification

## Purpose
TBD - created by archiving change m5-editor-and-modern-extras. Update Purpose after archive.
## Requirements
### Requirement: Fuzzy jump dialog invocation

The system SHALL open the fuzzy jump dialog when the user presses Ctrl+J (the default binding, overridable via `config.toml`), presenting a modal list of previously visited directories drawn in the standard NC dialog style over the panels. The dialog SHALL provide a single-line input field — a shared text field per `text-field-editing` — into which the user types a fuzzy match pattern, and Esc SHALL close the dialog without changing the active panel's directory.

#### Scenario: Ctrl+J opens the dialog

- **WHEN** the user presses Ctrl+J while a panel is focused
- **THEN** the fuzzy jump dialog opens as a modal box over the panels with an empty input field and the visited-directory list shown beneath it

#### Scenario: Esc closes without navigating

- **WHEN** the fuzzy jump dialog is open and the user presses Esc
- **THEN** the dialog closes and the active panel remains at its current directory unchanged

#### Scenario: Pattern edited mid-string re-narrows the list

- **WHEN** the pattern reads `prj` and the user presses Left, then types `o`
- **THEN** the pattern reads `proj` and the list is re-narrowed against `proj`

#### Scenario: Overridden binding still opens the dialog

- **WHEN** `config.toml` rebinds fuzzy jump to a different key and the user presses that key
- **THEN** the fuzzy jump dialog opens exactly as it would for the default Ctrl+J binding

### Requirement: Fuzzy matching of visited directories

The system SHALL filter the visited-directory list to entries whose path is a subsequence match for the typed pattern, updating the displayed list as each character is typed and as Backspace shortens the pattern. With an empty pattern the system SHALL show the full frecency-ranked list. Matching SHALL be performed in `filecommand-core` independently of the terminal so it is unit-testable.

#### Scenario: Typing narrows to subsequence matches

- **WHEN** the user types a pattern into the dialog input field
- **THEN** the list narrows to only those visited directories whose path contains the pattern's characters as an ordered subsequence

#### Scenario: Backspace re-widens the list

- **WHEN** the user presses Backspace to remove characters from the pattern
- **THEN** the list re-expands to include directories that match the shortened pattern

#### Scenario: Empty pattern shows all entries

- **WHEN** the dialog is open with an empty input field
- **THEN** the full list of visited directories is shown in frecency rank order

### Requirement: Frecency ranking

The system SHALL rank the visited-directory list by a frecency score combining recency and visit frequency, with more recently and more frequently visited directories ranked higher, so the most likely target appears near the top. The ranking computation SHALL live in `filecommand-core` and be deterministic under a pinned clock for testability. Recency SHALL be measured from wall-clock time. Each directory's last-visit time SHALL be recorded and persisted as Unix-epoch milliseconds, so that recency comparisons stay meaningful across application restarts. Animation and other in-session timing SHALL NOT depend on the wall clock. History entries loaded from a `history.json` written in the earlier session-relative timestamp format SHALL be treated as last visited at the time the history was loaded. A persisted last-visit time later than the current wall-clock time SHALL be treated as the current time.

#### Scenario: Frequency raises rank

- **WHEN** two directories match the pattern equally and one has been visited more times
- **THEN** the more frequently visited directory is ranked above the other

#### Scenario: Recency raises rank

- **WHEN** two directories have equal visit frequency and one was visited more recently
- **THEN** the more recently visited directory is ranked above the other

#### Scenario: Recency survives a restart

- **WHEN** in an earlier session that ran for several days the user visited `C:\old` on the first day and `C:\new` one minute before exiting, both directories have equal visit counts, and FileCommand is relaunched a few minutes later
- **THEN** the fuzzy jump dialog ranks `C:\new` above `C:\old`

#### Scenario: Legacy history file is migrated at load

- **WHEN** FileCommand starts with a `history.json` written in the earlier session-relative timestamp format
- **THEN** every loaded directory is treated as last visited at load time, so those directories rank among themselves by visit count, and visits recorded afterwards are stamped with wall-clock time

#### Scenario: Future last-visit time is clamped

- **WHEN** `history.json` holds a directory whose last-visit time is later than the current wall-clock time, for example after the system clock was set back
- **THEN** that directory is treated as visited at the current time and does not keep an inflated recency as time passes

### Requirement: Enter navigates the active panel

The system SHALL, when the user presses Enter on a highlighted directory in the fuzzy jump dialog, close the dialog and navigate the active panel to that directory, leaving the opposite panel unchanged. If the chosen directory no longer exists or cannot be read, the active panel SHALL show its inline panel read-error state rather than crashing.

#### Scenario: Enter jumps the active panel

- **WHEN** the user highlights a directory in the dialog and presses Enter
- **THEN** the dialog closes and the active panel changes its current directory to the selected path while the opposite panel is unaffected

#### Scenario: Chosen directory is missing

- **WHEN** the user presses Enter on a directory that has since been deleted or is inaccessible
- **THEN** the active panel displays its inline read-error state offering re-read/drive change and the application does not crash

### Requirement: Directory history persistence

The system SHALL record each directory the user navigates into in the frecency history and persist that history in `history.json` in the platform config directory, written atomically and shared with command history. The system SHALL load this history on startup so the fuzzy jump dialog reflects directories visited in prior sessions, and SHALL fall back to an empty history when the file is absent or malformed rather than failing to launch.

The directory history SHALL hold at most 1,000 entries. When recording a visit to a directory not yet in a full history, and when loading or merging a history larger than the cap, the system SHALL remove the entries with the lowest frecency score until the cap is met. It SHALL never remove the directory being visited.

Writes of `history.json` SHALL NOT be performed on the UI thread. Navigation SHALL hand the latest history to a background writer and return without waiting. The writer SHALL coalesce a burst of changes into one write, and SHALL write any unwritten change within 10 seconds of it being recorded. When the application quits, any change not yet written SHALL be written before the process exits. The exit SHALL wait for that final write for a bounded time of about 2 seconds, so that an unreachable profile directory cannot keep the process from exiting.

Before each write, the system SHALL re-read `history.json` and merge it with its own history so that concurrently running instances do not discard each other's entries:

- Directories SHALL be combined by path, keeping the larger visit count and the later last-visit time for a path present in both.
- Commands present only in the file SHALL be kept as older than this instance's commands, subject to the existing command-history cap.

Each write SHALL go to a uniquely named temporary file in the same directory, which SHALL be flushed to stable storage before it replaces `history.json`, so an interrupted write leaves either the previous or the new complete file.

#### Scenario: Navigation records history

- **WHEN** the user navigates the active panel into a directory
- **THEN** that directory's frecency entry is created or updated with an incremented visit count and refreshed last-visit time

#### Scenario: History survives restart

- **WHEN** the user visits directories, exits FileCommand, and relaunches
- **THEN** the fuzzy jump dialog lists those previously visited directories using their persisted frecency ranking

#### Scenario: Missing or malformed history file

- **WHEN** `history.json` is absent or cannot be parsed at startup
- **THEN** the fuzzy jump history loads as empty and the application launches normally without error

#### Scenario: History is capped by frecency

- **WHEN** the directory history holds 1,000 entries and the user navigates into a directory that is not in it
- **THEN** the entry with the lowest frecency score is removed, the new directory is recorded, and the history still holds 1,000 entries

#### Scenario: Navigation does not wait for the history write

- **WHEN** the user navigates the active panel into a directory
- **THEN** the navigation completes without the UI thread writing `history.json`, and the visit is written by the background writer afterwards

#### Scenario: A burst of navigation is written once

- **WHEN** the user navigates through 20 directories within one second and then pauses
- **THEN** `history.json` is written once after the pause, and that write contains all 20 visits

#### Scenario: Quitting flushes unwritten history

- **WHEN** the user navigates into `C:\work` and quits FileCommand before the pending change has been written
- **THEN** after the process exits, `history.json` contains the visit to `C:\work`

#### Scenario: Two instances keep each other's history

- **WHEN** two FileCommand instances share one profile directory, the first navigates into `C:\a`, the second navigates into `C:\b`, and both are then closed
- **THEN** `history.json` contains directory entries for both `C:\a` and `C:\b`

#### Scenario: Merge keeps the larger count and the later visit

- **WHEN** `history.json` records `C:\src` with visit count 9 last visited at time T1, and this instance's history records `C:\src` with visit count 4 last visited at a later time T2
- **THEN** the next write records `C:\src` with visit count 9 and last-visit time T2

#### Scenario: Another instance's commands are kept

- **WHEN** `history.json` contains the command `cd D:\build`, which this instance never ran, and this instance writes its history
- **THEN** the written command history still contains `cd D:\build`, ordered before this instance's own commands

#### Scenario: Interrupted write leaves a complete file

- **WHEN** the process is terminated while `history.json` is being written
- **THEN** `history.json` holds either its complete previous content or its complete new content, never a truncated or empty file

