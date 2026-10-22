## ADDED Requirements

### Requirement: Periodic directory change detection

While automatic refresh is enabled, the system SHALL check each eligible panel's current directory for changes every `refresh_interval` seconds (default 60) without user input, on a worker thread. An unchanged directory SHALL cause no state change and no redraw; a changed one SHALL re-read as "Automatic refresh is an in-place re-read" specifies. Both panels' active tabs are eligible; background tabs are not checked and keep their stale-on-activation refresh.

#### Scenario: A file created by another program appears

- **WHEN** the left panel shows `C:\build` with the cursor on `main.obj`, another program creates `C:\build\app.exe`, and the next automatic check runs
- **THEN** the left panel re-reads, lists `app.exe` in sorted position, and the cursor is still on `main.obj` with the scroll offset unchanged

#### Scenario: Nothing changed, nothing happens

- **WHEN** a panel's directory has not changed since its listing completed and an automatic check runs
- **THEN** the panel's entries, cursor, scroll offset, and selection are untouched, no `Reading…` indicator appears, and no frame is drawn as a result of the check

#### Scenario: Both panels are checked

- **WHEN** the left panel shows `C:\a` and the right panel shows `D:\b`, both listings are complete, and a check falls due
- **THEN** both directories are checked in the same round, and only a panel whose directory changed re-reads

#### Scenario: Background tabs are not checked

- **WHEN** the active panel has a background tab on `C:\logs` and another program writes a file there
- **THEN** no check is made for `C:\logs` while that tab is in the background, and the tab still re-reads on activation only if it was marked stale

### Requirement: Change detection compares a content digest

A check SHALL enumerate the directory's direct entries with the same reader and classification the panel listing uses, and SHALL compute a content digest over each entry's name, kind, size, modified time, hidden/system flag, and link flag together with the entry count. The digest SHALL be compared with the digest of the listing the panel shows; the directory counts as changed exactly when the two differ. A check whose directory cannot be read SHALL change nothing.

#### Scenario: A file modified in place is noticed

- **WHEN** a panel shows `notes.txt` with size 1,204 and another program appends to it so its size and modified time change, and the next automatic check runs
- **THEN** the panel re-reads and shows the new size and modified time for `notes.txt`

#### Scenario: A rename is noticed

- **WHEN** another program renames `draft.txt` to `final.txt` in a panel's directory and the next automatic check runs
- **THEN** the panel re-reads and lists `final.txt` in place of `draft.txt`

#### Scenario: An unreadable directory is left alone

- **WHEN** a panel shows a listing of `E:\photos`, the drive is removed, and an automatic check runs
- **THEN** the panel keeps showing its last listing with no error state, and the user's next navigation or Ctrl+R/F12 shows the normal inline read-error state

### Requirement: One outstanding check per panel

At most one check per panel SHALL be outstanding at any time. A check that has not answered when the next interval falls due SHALL delay that panel's next check until it has answered; the other panel's checks SHALL continue on schedule.

#### Scenario: A slow share does not pile up checks

- **WHEN** the left panel's directory is on a share whose enumeration takes longer than `refresh_interval` and the right panel shows a local directory
- **THEN** no second check for the left panel starts until the first has answered, and the right panel is still checked every interval

### Requirement: Automatic refresh is an in-place re-read

A re-read triggered by an automatic check SHALL be the in-place refresh a stale tab receives on activation, not a navigation: cursor and scroll position restored per panel-navigation "Same-directory re-read preserves cursor and scroll position", the identity-keyed selection kept for entries still present, sort mode and direction kept, and the git-info query re-issued. It SHALL NOT record a fuzzy-jump history visit or write any configuration file.

#### Scenario: Selection survives an automatic refresh

- **WHEN** `a.txt` and `c.txt` are selected in a panel, another program adds `b.txt`, and the next check re-reads the panel
- **THEN** `a.txt` and `c.txt` are still selected and `b.txt` is not

#### Scenario: The refresh is not a navigation

- **WHEN** an automatic check re-reads a panel
- **THEN** the directory's fuzzy-jump frecency entry is not created, re-ranked, or re-stamped, and `history.json` and `config.toml` are not written

#### Scenario: Sort mode is kept

- **WHEN** a panel sorted by Size descending is re-read by an automatic check
- **THEN** the refreshed listing is sorted by Size descending and the header still shows the arrow on the Size column

### Requirement: Automatic refresh never disturbs an interaction

A check SHALL be issued for a panel only while the panels are the active screen with no dialog, menu, mouse drag, or type-ahead in progress, and only for a panel whose listing is complete without a read error in Full or Brief display mode. A result SHALL be applied only if those conditions still hold on arrival, the panel still shows the checked directory, and the result is the panel's current outstanding check; otherwise it SHALL be dropped and the next interval tries again.

#### Scenario: A confirmation dialog is never undermined

- **WHEN** the delete confirmation for `report.txt` is open, another program deletes `report.txt`, and an automatic check falls due or its result arrives
- **THEN** the panel is not re-read while the dialog is open, the dialog still names `report.txt`, and confirming it reports that the file is missing through the normal error path rather than deleting a different file

#### Scenario: Checks resume after the dialog closes

- **WHEN** the dialog from the previous scenario is cancelled and the next interval elapses
- **THEN** the check runs, finds the directory changed, and the panel re-reads without `report.txt`

#### Scenario: A streaming listing is not checked

- **WHEN** a panel is still showing `Reading… N` for a large directory when a check falls due
- **THEN** no check is issued for that panel in that round

#### Scenario: Tree, Info, and QuickView panels are skipped

- **WHEN** the left panel is in Tree mode and the right panel in Full mode and a check falls due
- **THEN** only the right panel's directory is checked

#### Scenario: Viewer, editor, and running jobs are never interrupted

- **WHEN** the built-in viewer or editor is open, or a file-operation dialog or job is on screen, and an interval elapses
- **THEN** no check is issued and no panel re-reads until the panels are the active screen again

### Requirement: Configurable refresh interval

The system SHALL read `refresh_interval` from `config.toml`'s top-level settings as a non-negative integer number of seconds, defaulting to `60`. `0` SHALL disable automatic checks entirely, leaving the event loop with no refresh deadline. A value that is not a non-negative integer SHALL produce the standard startup warning naming the line, and the default SHALL apply. The interval SHALL be measured from when the previous round of checks was issued.

#### Scenario: Default interval

- **WHEN** `config.toml` has no `refresh_interval` line
- **THEN** checks run about every 60 seconds

#### Scenario: Automatic refresh turned off

- **WHEN** `config.toml` contains `refresh_interval = 0`
- **THEN** no check is ever issued, and an idle screen with no other deadline draws no frame for as long as it sits idle

#### Scenario: Custom interval

- **WHEN** `config.toml` contains `refresh_interval = 15`
- **THEN** checks run about every 15 seconds

#### Scenario: Uninterpretable value is reported

- **WHEN** `config.toml` contains `refresh_interval = fast` on line 6
- **THEN** checks run at the default 60-second interval and the startup-warning dialog names `config.toml` line 6 and states that the value must be a non-negative whole number of seconds
