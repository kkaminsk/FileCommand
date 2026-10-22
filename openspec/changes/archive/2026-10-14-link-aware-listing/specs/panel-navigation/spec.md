## ADDED Requirements

### Requirement: Directory links list as directories

When a directory listing contains a link (a junction or symbolic link on Windows, a symbolic link elsewhere), the system SHALL classify the entry by following the link to its target:

- A link whose target is a directory SHALL be listed, sorted, and rendered exactly as a directory: the directory style, `▶SUB-DIR◀` in the Size column, and Enter descends into it.
- A link whose target is a file SHALL be listed as a file, showing the target's size.
- A link whose target cannot be reached (dangling, or not followable) SHALL be listed as a file entry that Enter does not navigate into. It SHALL NOT cause the listing to fail.

In every case, the entry's name, date and time, and hidden state SHALL be those of the link entry itself. The system SHALL record internally that the entry is a link. No link-specific marker is required in the entry row.

Descending into a directory link SHALL make the link's own path (the containing directory joined with the link's name) the panel's current directory. Parent navigation from there SHALL return to the directory that contains the link.

Deleting a directory link removes only the link (the scenario "Delete removes the link, not the target" under file-operations requirement "Reparse-point (symlink/junction) semantics"), so the delete confirmation for it SHALL NOT raise the second confirmation that a non-empty directory requires.

#### Scenario: A junction to a directory lists as a directory
- **WHEN** a panel lists `C:\proj\node_modules`, which contains a junction `lodash` pointing to an existing directory
- **THEN** `lodash` is listed as a directory: it renders in the directory style with `▶SUB-DIR◀` in the Size column and sorts among the directories

#### Scenario: Enter descends into a directory link
- **WHEN** the cursor is on the junction `lodash` in `C:\proj\node_modules` and Enter is pressed
- **THEN** the panel's current directory becomes `C:\proj\node_modules\lodash` and a listing of the link target's contents begins

#### Scenario: Parent navigation from inside a link returns to the link's directory
- **WHEN** the panel entered `C:\proj\node_modules\lodash` through the junction and the user presses Ctrl+PgUp (or Enter on `..`)
- **THEN** the panel navigates to `C:\proj\node_modules`, not to the parent of the link's target

#### Scenario: A directory symbolic link behaves like a junction
- **WHEN** a listed entry is a symbolic link whose target is a directory
- **THEN** it is listed as a directory and Enter descends into it

#### Scenario: A link to a file lists as a file with the target's size
- **WHEN** a listed entry is a symbolic link to a 4,096-byte file
- **THEN** it is listed as a file and its Size column shows 4,096

#### Scenario: A dangling link does not fail the listing
- **WHEN** a directory contains a junction `old` whose target directory no longer exists, alongside other entries
- **THEN** the listing completes with every entry shown, `old` is listed as a file entry rather than a directory, and pressing Enter on `old` does not change the panel's directory

#### Scenario: Deleting a directory link asks once
- **WHEN** the cursor is on the junction `lodash`, whose target directory contains files, and the user presses F8 and confirms
- **THEN** no second "non-empty directory" confirmation is shown, and the delete job starts for the link

### Requirement: One unreadable entry does not fail a listing

When the system reads a directory and the metadata of an individual entry cannot be read, it SHALL skip that entry, count it, and continue the listing. It SHALL NOT fail the whole listing because of it. A failure to enumerate the directory itself, rather than a single entry, SHALL still put the panel into the inline read-error state.

When a listing completes after skipping one or more entries, the panel's mini-status line SHALL report how many were skipped: `1 entry could not be read`, or `N entries could not be read` with N comma-grouped. The message SHALL remain until that panel next lists a directory. It is shown in place of the highlighted entry's details that "Streaming listing mini-status" otherwise restores on completion.

#### Scenario: One unreadable entry is skipped and reported
- **WHEN** a panel lists a directory of 50 entries and one entry's metadata cannot be read
- **THEN** the other 49 entries are listed and the panel is not in the read-error state
- **AND** when the listing completes the mini-status line reads `1 entry could not be read`

#### Scenario: The message clears on the next listing
- **WHEN** the mini-status shows `2 entries could not be read` and the user presses Ctrl+R, and this time every entry is read
- **THEN** after the re-read completes the mini-status shows the highlighted entry's name, size, date, and time again

#### Scenario: A directory that cannot be enumerated still shows the read-error state
- **WHEN** a panel attempts to list a directory and enumerating the directory itself fails (for example, access denied or the drive was removed)
- **THEN** the panel enters the inline read-error state exactly as before

## MODIFIED Requirements

### Requirement: Streaming listing mini-status

While a directory listing is still being read, the mini-status line SHALL show `Reading… N` with a running, comma-grouped entry count that updates as chunks arrive, and SHALL revert to the normal name/size/date/time line when the read completes. The one exception is a listing that skipped unreadable entries: on completion its mini-status SHALL show the skipped count instead, as "One unreadable entry does not fail a listing" defines. Entries SHALL be inserted in sorted position as they arrive, and the panel SHALL remain interactive while the listing is incomplete, with the cursor held on the first row until the user moves it.

#### Scenario: Mini-status shows running count while reading

- **WHEN** a directory listing is incomplete and 12345 entries have arrived
- **THEN** the mini-status line SHALL read `Reading… 12,345`

#### Scenario: Mini-status reverts on completion

- **WHEN** a directory listing completes and no entry was skipped as unreadable
- **THEN** the mini-status line SHALL revert to showing the highlighted entry's name, size, date, and time

#### Scenario: Cursor holds on first row during streaming

- **WHEN** entries continue to arrive and the user has not moved the cursor
- **THEN** the cursor SHALL remain on the first row while newly arriving entries are inserted in sorted position

#### Scenario: Partial listing is interactive

- **WHEN** a directory listing is still streaming
- **THEN** cursor-movement commands SHALL operate on the entries already received without waiting for the read to complete

#### Scenario: Completion after skipped entries shows the count

- **WHEN** a directory listing completes after skipping 2 unreadable entries
- **THEN** the mini-status line reads `2 entries could not be read` instead of the highlighted entry's details
