## MODIFIED Requirements

### Requirement: Background repository detection

The system SHALL detect the git repository enclosing a panel's current directory on a dedicated worker thread using `git2` (libgit2), and the single-threaded UI event loop SHALL NOT block on the repository detection or status query. Detection and status results SHALL be delivered to the reducer as job events, and a fresh query SHALL be issued when the panel navigates to a new directory or is re-read (Ctrl+R), since v1 performs no filesystem watching — except for a directory inside a repository that is marked "no info" for the session, for which no query is issued (see "Silent absence on timeout and stale-result discarding").

#### Scenario: Directory inside a repository

- **WHEN** a panel's current directory lies within a git working tree
- **THEN** the worker thread resolves the enclosing repository and reports its branch name and per-file status back to the reducer without the UI thread waiting on the libgit2 call

#### Scenario: Directory outside any repository

- **WHEN** a panel's current directory is not inside any git working tree
- **THEN** the query resolves to "no git info" and the panel renders no branch suffix and no status-marker column

#### Scenario: Query re-issued on navigation

- **WHEN** the panel changes directory or the user triggers a re-read (Ctrl+R), and the panel's new current directory is not inside a repository marked "no info" for the session
- **THEN** a new git query is issued for the panel's new current directory rather than reusing a prior result

### Requirement: Pathspec-scoped status queries

The system SHALL scope git status queries to the panel's current directory via pathspec rather than querying the repository as a whole, and SHALL NOT enumerate the contents of untracked directories. The panel directory's path SHALL be matched literally: characters that pathspec patterns treat specially — `[`, `]`, `*`, `?`, `#`, spaces, and a leading `!` — in any directory name along that path SHALL NOT be interpreted as pattern syntax, so status is reported for a directory with such a name exactly as for any other directory, and the query stays scoped to that directory.

#### Scenario: Status limited to panel directory

- **WHEN** git status is computed for a panel deep inside a large repository
- **THEN** the query is pathspec-scoped to the panel directory and does not report status for files outside that directory

#### Scenario: Untracked directory not expanded

- **WHEN** the panel directory contains an untracked subdirectory
- **THEN** the subdirectory is marked `?` but the files inside it are not individually enumerated for status

#### Scenario: Directory names are matched literally

- **WHEN** the panel is inside a repository directory named `foo[1]` that contains a modified tracked file `a.txt` and an untracked subdirectory `new`
- **THEN** `a.txt` shows `M` and `new` shows `?`, exactly as they would in a directory named `foo1`

#### Scenario: Pattern characters in a nested directory name

- **WHEN** the panel is inside the repository directory `src/foo[1]`, and a sibling directory `src/foo1` also contains modified files
- **THEN** the files of `src/foo[1]` show their markers and nothing from `src/foo1` is reported for the panel

#### Scenario: A leading exclamation mark is not a negation

- **WHEN** the panel is inside a repository directory named `!old` that contains a modified tracked file
- **THEN** that file shows `M`

#### Scenario: A leading hash does not widen the query

- **WHEN** the panel is inside a top-level repository directory named `#notes`
- **THEN** the status query is scoped to `#notes` rather than walking the whole repository, and its entries show their markers

### Requirement: Silent absence on timeout and stale-result discarding

The system SHALL degrade to "no git info" silently on timeout, discarding the stale result and marking the repository as "no info" for the remainder of the session while leaving the uncancellable worker call to finish. While a repository is marked "no info", navigating to, re-reading, or swapping a panel into any directory inside that repository's working tree SHALL NOT start another git query, and the panel SHALL show no branch suffix, no marker column, and no error for it; when the repository could not be located before the timeout, the queried directory and everything beneath it SHALL be marked instead. A timeout SHALL mark the repository even when the panel has already moved elsewhere. Each panel SHALL have at most one git query running at a time: a navigation while a query is running SHALL wait for that query to report and then query only the panel's most recent directory, a waiting request superseded by a newer navigation SHALL never be started, and a waiting request for a directory inside the repository that the running query has just timed out on SHALL never be started either. Results keyed to a directory or generation the panel has since moved past SHALL be dropped and SHALL NOT alter panel rendering.

#### Scenario: Timeout degrades silently

- **WHEN** a git status query exceeds its timeout
- **THEN** the result is discarded, the repository is treated as "no info" for the session, and the panel shows no error and no indicator

#### Scenario: Late result after navigation is dropped

- **WHEN** a git result arrives for a directory the panel has already navigated away from
- **THEN** the result is discarded via its directory/generation key and the current panel rendering is unchanged

#### Scenario: Timed-out repository is not queried again

- **WHEN** a git query for `D:\mono\src` in the repository rooted at `D:\mono` has timed out, and the user then navigates the panel to `D:\mono\docs` or presses Ctrl+R there
- **THEN** no git query is started, and the panel shows no branch suffix and no marker column

#### Scenario: Swapping a panel into a timed-out repository

- **WHEN** the repository at `D:\mono` is marked "no info" and the user presses Ctrl+U while the other panel shows `D:\mono\lib` with branch info it had resolved before the timeout
- **THEN** no git query is started for that panel after the swap, and it shows no branch suffix and no marker column

#### Scenario: Other repositories are still queried

- **WHEN** the repository at `D:\mono` is marked "no info" and the user navigates to `C:\work\app`, inside a different repository
- **THEN** a git query is started for `C:\work\app` and its branch suffix and markers appear when it resolves

#### Scenario: Timeout recorded after the panel moved on

- **WHEN** the user leaves `D:\mono\src` before its git query times out, and the timeout is reported afterwards
- **THEN** the repository at `D:\mono` is still marked "no info", and a later navigation into it starts no git query

#### Scenario: Repository not located before the timeout

- **WHEN** a git query for `\\server\share\proj` times out before the enclosing repository was even located
- **THEN** `\\server\share\proj` and every directory beneath it are treated as "no info" for the session

#### Scenario: Rapid navigation does not pile up queries

- **WHEN** a panel's git query is still running and the user navigates through three more directories in quick succession, and the running query then completes within its timeout
- **THEN** no second query runs for that panel while the first is running, and once the first reports, only the last of the three directories is queried

#### Scenario: Waiting query inside the timed-out repository is not started

- **WHEN** a panel's git query for `D:\mono\src` is still running, the user navigates to `D:\mono\docs` while it runs, and the running query then times out
- **THEN** no git query is started for `D:\mono\docs`, and the panel shows no branch suffix and no marker column there
