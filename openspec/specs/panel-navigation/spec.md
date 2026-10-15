# panel-navigation Specification

## Purpose
TBD - created by archiving change m1-shell. Update Purpose after archive.
## Requirements
### Requirement: Panel state model

Each panel SHALL maintain its own state consisting of a current directory path, an ordered entry list, a cursor position within that list, and a sort order. Panel state SHALL live in `filecommand-core` with no dependency on the terminal, and all mutations SHALL flow through `core::update`.

#### Scenario: Panel exposes current directory and entries

- **WHEN** a panel has finished reading a directory
- **THEN** its state SHALL expose the current directory path, the list of entries for that directory, and a cursor index into that list

#### Scenario: Entries are ordered by the panel's sort order

- **WHEN** a panel's sort order is Name
- **THEN** entries SHALL be presented sorted by name, with the `..` parent entry (when present) ordered first

#### Scenario: Cursor stays within bounds

- **WHEN** the cursor index would move before the first entry or past the last entry
- **THEN** `core::update` SHALL clamp the cursor to a valid entry index and leave panel state otherwise unchanged

### Requirement: Full display mode layout

The system SHALL render each panel in Full display mode with a double-line border, the current directory path centered in the top border, a column header row, entry rows, and a mini-status line inside the bottom border. The columns SHALL be the widest set from the ladder `Name+Size+Date+Time → Name+Size+Date → Name+Size → Name` that keeps the Name column at least 12 display cells wide, dropping columns rightmost-first (Time, then Date, then Size) as the panel narrows and restoring them in reverse as it widens; the column header row SHALL show exactly the columns currently rendered. A rendered row SHALL be laid out as the Name cell, a `│` separator, a 9-cell Size value, one space, an 8-cell Date value (`MM-DD-YY`), one space, and a 5-cell Time value (`HH:MM`), so that every rendered Size, Date, and Time value appears untruncated and the row fills the panel interior exactly. At the 80×24 nominal size with the default 50/50 split, all four columns render. The active panel's path title SHALL render inverse (black on cyan) and the inactive panel's title SHALL render cyan on blue.

#### Scenario: All four columns at the nominal size

- **WHEN** a panel renders Full mode at terminal size 80×24 with the default 50/50 split
- **THEN** the header row reads `Name | Size | Date | Time` and all four columns render, with the Name column at least 12 display cells wide

#### Scenario: Values render untruncated at the nominal size

- **WHEN** a panel renders Full mode at 80×24 with the default split and an entry modified on 2026-06-27 at 23:30
- **THEN** the row shows `06-27-26` and `23:30` complete, and a directory row shows `▶SUB-DIR◀` complete, with no blank cells between the Time value and the right border

#### Scenario: Sizes are right-aligned

- **WHEN** two file entries of 87 bytes and 35 KiB render in Full mode
- **THEN** `87` and `35K` are right-aligned within the Size column so their last digits share a cell column

#### Scenario: Time drops first on a narrowing panel

- **WHEN** a panel narrows to where rendering all four columns would leave the Name column under 12 display cells
- **THEN** the Time column and its header are dropped, and the remaining columns render with Name at 12 or more cells

#### Scenario: Name-only at minimum panel width

- **WHEN** a panel is at the 20-column minimum width
- **THEN** only the Name column renders, spanning the panel interior, and the header row shows only `Name`

#### Scenario: Active panel title is inverse

- **WHEN** a panel is the active panel
- **THEN** its centered path title in the top border SHALL be rendered with the `panel.title.active` role (black on cyan)

#### Scenario: Inactive panel title is not inverse

- **WHEN** a panel is not the active panel
- **THEN** its centered path title SHALL be rendered with the `panel.title.inactive` role (cyan on blue)

#### Scenario: Sort column shows an indicator

- **WHEN** Full mode renders the column header row and the panel is sorted by Name
- **THEN** the header SHALL show a `↓`/`↑` sort indicator next to the active sort column's label

### Requirement: Entry row rendering

The system SHALL render entry rows so directories appear in the bright-white directory style and files in the file style, with `▶UP--DIR◀` shown for the `..` entry and `▶SUB-DIR◀` shown complete in the Size column for directories; numeric sizes SHALL be right-aligned in that column. Entries flagged hidden — on Windows those carrying the hidden or system attribute, elsewhere names beginning with `.` — SHALL render their name with the `panel.hidden` role instead of the file or directory style; the `..` entry is never hidden. Style precedence SHALL be: drag-target highlight, then cursor bar, then selected, then hidden, then directory/file. The entry under the cursor SHALL render as a full-width inverse bar using the `panel.cursor` role. Rendering SHALL use only ANSI-16 named color roles and CP437 box-drawing/geometric glyphs, with no emoji, Nerd Font, or file-type icons.

#### Scenario: Directory entry styling

- **WHEN** an entry is a directory
- **THEN** its name SHALL render in the `panel.directory` style and its Size column SHALL read `▶SUB-DIR◀` in full

#### Scenario: Parent entry styling

- **WHEN** the entry list contains the parent `..` entry
- **THEN** it SHALL render as `▶UP--DIR◀` in the `panel.directory` style

#### Scenario: Hidden entries render dimmed

- **WHEN** a directory listing contains `desktop.ini` (hidden attribute) and `.git` (hidden directory)
- **THEN** both names render with the `panel.hidden` role, `.git` still shows `▶SUB-DIR◀`, and neither entry's position in the sorted listing changes

#### Scenario: Selected hidden entry shows selected style

- **WHEN** a hidden file is selected with Ins
- **THEN** its row renders with the `panel.selected` role, not `panel.hidden`

#### Scenario: Cursor bar wins

- **WHEN** the panel cursor is on a hidden entry
- **THEN** that row SHALL render as the full-width `panel.cursor` inverse bar

#### Scenario: Cursor row is an inverse bar

- **WHEN** an entry is under the panel cursor
- **THEN** that row SHALL render as a full-width inverse bar using the `panel.cursor` role

### Requirement: Cursor movement

The system SHALL move the active panel's cursor in response to movement commands (up, down, and page/home/end movement) without leaving the current directory, updating the mini-status line to reflect the newly highlighted entry.

#### Scenario: Move cursor down

- **WHEN** the active panel has focus and a move-down command is issued
- **THEN** the cursor SHALL advance to the next entry and the panel SHALL remain in the same directory

#### Scenario: Mini-status reflects highlighted entry

- **WHEN** the cursor moves to a completed-listing entry
- **THEN** the mini-status line SHALL show that entry's name, size, date, and time

### Requirement: Tab switches the active panel

The system SHALL treat exactly one panel as active at a time, and the Tab key SHALL move focus to the other panel. Switching the active panel SHALL update which panel's title renders inverse and which panel receives subsequent movement and navigation commands.

#### Scenario: Tab moves focus to the other panel

- **WHEN** the left panel is active and Tab is pressed
- **THEN** the right panel SHALL become active and its path title SHALL render inverse while the left panel's title reverts to the inactive style

#### Scenario: Subsequent commands target the newly active panel

- **WHEN** the active panel has just changed via Tab
- **THEN** subsequent cursor-movement and Enter/parent-navigation commands SHALL apply to the newly active panel

### Requirement: Enter descends into a directory

When the cursor is on a directory entry, the Enter key SHALL make the active panel navigate into that directory: its current directory becomes the selected directory, a new listing is started for it, and the cursor resets to the first entry.

#### Scenario: Enter on a subdirectory

- **WHEN** the active panel's cursor is on a subdirectory and Enter is pressed
- **THEN** the panel's current directory SHALL become that subdirectory and a listing of its contents SHALL begin

#### Scenario: Enter on the parent entry

- **WHEN** the active panel's cursor is on the `..` entry and Enter is pressed
- **THEN** the panel SHALL navigate to the parent directory

#### Scenario: Cursor resets on descend

- **WHEN** the active panel navigates into a new directory
- **THEN** the cursor SHALL be positioned on the first entry of the new listing

### Requirement: Parent-directory navigation

The system SHALL navigate the active panel to its parent directory when Ctrl+PgUp is pressed, and when Backspace is pressed while the command line is empty. When the current directory is already a filesystem root with no parent, the navigation SHALL be a no-op.

#### Scenario: Ctrl+PgUp goes to the parent

- **WHEN** the active panel is in a non-root directory and Ctrl+PgUp is pressed
- **THEN** the panel SHALL navigate to its parent directory and begin listing it

#### Scenario: Backspace on empty command line goes to the parent

- **WHEN** the command line buffer is empty and Backspace is pressed
- **THEN** the active panel SHALL navigate to its parent directory

#### Scenario: Parent navigation at a root is a no-op

- **WHEN** the active panel is at a filesystem root with no parent and a parent-navigation command is issued
- **THEN** the panel SHALL remain in the current directory unchanged

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

### Requirement: Viewport scrolling keeps the cursor visible

In Full display mode the panel body SHALL render a window of consecutive positions from the visible entry list, starting at the panel's scroll offset, and the cursor's position SHALL always lie within that window. The window SHALL move only when a cursor movement would otherwise place the cursor outside it, and SHALL then move the minimum distance that restores visibility — single-step movements shift the window by one line, `Home` pins the window to the top of the list, `End` pins it to the bottom, and jump-style cursor changes (paging, type-ahead jump, find-file's cursor settle) land with the cursor inside the window.

#### Scenario: Cursor moving below the bottom edge scrolls one line

- **WHEN** the cursor is on the last visible row of a Full-mode panel whose list extends beyond the window and the user presses Down
- **THEN** the window shifts down by exactly one line, the cursor stays on the last visible row, and the entry above the old window's first row is no longer shown

#### Scenario: Cursor moving above the top edge scrolls one line

- **WHEN** the window starts below the top of the list, the cursor is on the first visible row, and the user presses Up
- **THEN** the window shifts up by exactly one line and the cursor stays on the first visible row

#### Scenario: Window does not move while the cursor is inside it

- **WHEN** the cursor moves between two rows that are both already visible
- **THEN** the scroll offset is unchanged and no other rows enter or leave the window

#### Scenario: Jump movements land the cursor in view

- **WHEN** a type-ahead jump moves the cursor to an entry outside the current window
- **THEN** the scroll offset changes so the cursor's entry is rendered inside the window

#### Scenario: Home and End pin the window

- **WHEN** the user presses Home, and later End, in a list longer than the window
- **THEN** after Home the window starts at the first position with the cursor on it, and after End the window ends at the last position with the cursor on it

### Requirement: Scroll offset is core panel state

Each panel's scroll offset SHALL live in `filecommand-core` panel state (with Tree mode's offset in its tree state), and all offset changes SHALL flow through `core::update` — the renderer only reads it. The offset SHALL be a position in the quick-filter-narrowed visible list, not a raw entry index. Core SHALL derive each panel's body row count from state it already holds (terminal size from `Resize`, the panel split, the panel's display mode, and tab-strip visibility) — it SHALL NOT query the terminal — and SHALL re-clamp the offset so the cursor stays visible after every mutation that can move the cursor or change the list: cursor movement, quick-filter edits, re-sort, streamed listing updates, directory load completion (including find-file's deferred cursor settle and the same-directory re-read anchor restore), tab restore, and terminal resize.

#### Scenario: Quick-filter narrowing re-clamps the offset

- **WHEN** a quick-filter keystroke narrows the visible list so the current offset would leave the cursor's entry outside the window
- **THEN** the offset is re-clamped in the same reducer step so the cursor's entry is visible

#### Scenario: Re-sort keeps the cursor's entry in view

- **WHEN** the sort mode changes and the cursor re-anchors to the same entry at a new position
- **THEN** the offset is re-clamped so that entry is inside the window

#### Scenario: Terminal resize re-clamps

- **WHEN** the terminal shrinks so the current offset would leave the cursor below the new, shorter window
- **THEN** the next reducer step after the `Resize` re-clamps the offset and the cursor is visible in the new geometry

#### Scenario: Tab restore re-clamps against the current viewport

- **WHEN** a tab stashed with a scroll offset is restored while the panel's body height differs from when it was stashed
- **THEN** the restored offset is re-clamped so the restored cursor is visible at the current height

#### Scenario: Streamed listing keeps the top pinned until the user moves

- **WHEN** entries stream into a freshly loaded directory and the user has not moved the cursor
- **THEN** the offset stays 0 with the cursor pinned to the first entry, exactly as the cursor itself already behaves

#### Scenario: Re-read anchor restore re-clamps against the shorter list

- **WHEN** a same-directory re-read restores a scroll offset that would leave blank rows below the end of the now-shorter list
- **THEN** the offset is re-clamped in the same reducer step so the window ends at the last entry and the cursor remains visible

### Requirement: Scrollbar indicator on overflow

When (and only when) a panel's visible list is longer than its body window, the body's entry rows in the panel's right border column SHALL render a vertical scrollbar in place of the double-line `║` glyphs on those rows — this requirement takes precedence, on overflow, over the unbroken double-line border described by "Full display mode layout". The scrollbar SHALL use only CP437 glyphs — `░` for the track and `█` for the thumb — styled by the `panel.scrollbar` theme role, with thumb length proportional to the visible fraction (minimum one cell) and thumb position proportional to the scroll offset, touching the track's top exactly when the offset is 0 and its bottom exactly when the last position is visible. The scrollbar SHALL occupy only body entry rows: never the top border, tab-strip row, column-header row, or the bottom border and its mini-status. When the list fits the window, the right border SHALL render exactly as it does today, byte-identical.

#### Scenario: Overflowing list shows the scrollbar

- **WHEN** a Full-mode panel's visible list has more entries than the body has rows
- **THEN** the body entry rows of the right border render a `░` track with a `█` thumb in the `panel.scrollbar` role, and the border rows above and below the body are unchanged

#### Scenario: Fitting list keeps the plain border

- **WHEN** the visible list fits entirely within the body window
- **THEN** the right border renders as unbroken double-line `║` glyphs, identical to the rendering before this change

#### Scenario: Thumb touches the ends at the extremes

- **WHEN** the offset is 0, and later the window shows the last list position
- **THEN** the thumb's top cell is the track's top cell in the first case and the thumb's bottom cell is the track's bottom cell in the second

#### Scenario: Thumb reflects position mid-list

- **WHEN** the window sits partway through a long list
- **THEN** the thumb sits strictly between the track's ends, at a row proportional to the offset

### Requirement: Same-directory re-read preserves cursor and scroll position

When a panel re-reads the directory it is already displaying — after a file-operation job completes or is cancelled, on Ctrl+R or the Left/Right menu's Re-read item, or on a stale background tab's activation refresh — the system SHALL restore the cursor to the entry it was on if that entry is still listed; otherwise to the first surviving entry at or after the cursor's previous row; otherwise to the nearest surviving entry before it. The scroll offset SHALL be restored to its previous value and then re-clamped so the window does not extend past the end of the new list and the cursor remains visible. The restored position SHALL hold while the re-read streams in — the cursor SHALL NOT transiently jump to the first entry — and after completion. Navigation into a different directory SHALL continue to reset the cursor and offset to the top. For same-directory re-reads this requirement takes precedence over "Streamed listing keeps the top pinned until the user moves".

#### Scenario: Deleting the cursored file lands on the next entry

- **WHEN** the cursor is on `b.txt` in a listing `a.txt`, `b.txt`, `c.txt` and the user confirms a delete of `b.txt` that completes
- **THEN** after the automatic re-read the cursor is on `c.txt` and the scroll offset is unchanged

#### Scenario: Deleting the last entry lands on the previous one

- **WHEN** the cursor is on the last entry of a listing and the user confirms a delete of that entry that completes
- **THEN** after the re-read the cursor is on the entry that preceded it, now the last entry

#### Scenario: Multi-selection delete lands on the first survivor at or after the cursor

- **WHEN** a listing `a`, `b`, `c`, `d`, `e` has `b`, `c`, `d` selected with the cursor on `c`, and a delete of the selection completes
- **THEN** after the re-read the cursor is on `e`

#### Scenario: Scrolled window stays put

- **WHEN** a 60-entry listing is shown in a 20-row body with scroll offset 30 and the cursor on position 40, and a delete of the cursored entry completes
- **THEN** after the re-read the scroll offset is still 30 and the cursor is on position 40, which is the entry formerly at position 41

#### Scenario: Deleting at the very bottom pulls the window up by one

- **WHEN** a 60-entry listing is shown in a 20-row body with scroll offset 40 and the cursor on the last entry, and a delete of that entry completes
- **THEN** after the re-read the list has 59 entries, the scroll offset is re-clamped to 39, the cursor is on the new last entry, and no blank trailing row is shown

#### Scenario: Ctrl+R keeps the cursor's entry

- **WHEN** the cursor is on `report.txt` partway down a listing and the user presses Ctrl+R
- **THEN** after the re-read the cursor is on `report.txt` and the scroll offset is unchanged

#### Scenario: No jump while the re-read streams

- **WHEN** a same-directory re-read is in progress and its first streamed chunk contains the anchored entry
- **THEN** the cursor is on that entry as soon as the chunk is applied, not on the first entry

#### Scenario: Navigating to a different directory still starts at the top

- **WHEN** the cursor is partway down a listing and the user presses Enter on a subdirectory
- **THEN** the new listing starts with the cursor on its first entry and the scroll offset at 0

### Requirement: Swap panels (Ctrl+U)

The system SHALL exchange the left and right panels' complete state — directory, tabs, listing, cursor, scroll offset, selection, sort mode, filter, display mode, and git info — on Ctrl+U or Commands → Swap panels, keeping the active side and the split percentage unchanged, without re-reading either directory. A swap requested while either panel's listing is still streaming SHALL be ignored. After a swap the system SHALL re-issue each panel's pending Info-mode and git-info queries so no reply from before the swap is applied to the wrong panel.

#### Scenario: Swap keeps focus on the same side
- **WHEN** the left panel is active showing `C:\a` and the right shows `D:\b`, and the user presses Ctrl+U
- **THEN** the left panel shows `D:\b`, the right shows `C:\a`, the left panel is still active, and the command-line prompt reads `D:\b>`

#### Scenario: Swap carries tabs and selection
- **WHEN** the right panel has three tabs and two selected entries and the user presses Ctrl+U
- **THEN** the left panel now has those three tabs and the same two selected entries

#### Scenario: Swap refused while reading
- **WHEN** the right panel's listing is still streaming and the user presses Ctrl+U
- **THEN** nothing changes

#### Scenario: Git info follows the swap
- **WHEN** the left panel is inside a git repository and the user presses Ctrl+U
- **THEN** the right panel's border shows that repository's branch suffix after its re-issued query resolves, and the left panel shows none

### Requirement: Superseded directory listings are discarded and cancelled

Every directory listing the system starts for a panel SHALL be tagged with a fresh request id that becomes that panel's current listing request, and every streamed batch of entries, completion notice, and failure notice SHALL carry the id of the listing that produced it. The system SHALL apply a listing result only when its id matches the panel's current listing request and SHALL silently discard every other listing result, so that a listing superseded by a navigation, a re-read, a drive change, a file-operation refresh, or any other new listing of the same panel never adds entries to, completes, or fails the listing that replaced it. When a listing is superseded while it is still being read, the system SHALL signal its reader to stop; the reader SHALL stop at its next check and SHALL send no further entries, completion notice, or failure notice.

#### Scenario: Backspace out of a slow directory mid-listing

- **WHEN** the active panel enters `C:\big` and, while that listing is still streaming, the user presses Backspace on an empty command line to return to `C:\`
- **THEN** the panel shows only the entries of `C:\` — no entry from `C:\big`, not even `C:\big`'s `..` entry — and the `Reading… N` count covers only `C:\`'s entries
- **AND** the read of `C:\big` is signalled to stop

#### Scenario: cd elsewhere mid-listing

- **WHEN** the panel's listing of `D:\logs` is still streaming and the user runs `cd C:\Work` from the command line
- **THEN** only `C:\Work`'s entries appear in the panel, and none of `D:\logs`'s entries arrive later

#### Scenario: Superseded completion does not complete the new listing

- **WHEN** a superseded listing's completion notice arrives after the panel has started reading a different directory
- **THEN** the mini-status keeps showing `Reading… N` for the new listing until the new listing's own completion arrives

#### Scenario: Superseded failure does not mark the new listing failed

- **WHEN** the read of a directory the user has already left fails with access denied after the panel started reading another directory
- **THEN** no error is shown for the current directory and its listing continues normally

#### Scenario: Two quick re-reads do not duplicate entries

- **WHEN** the user presses Ctrl+R twice in quick succession on a directory with 500 entries
- **THEN** after both reads finish the panel lists each of the 500 entries exactly once

#### Scenario: Superseded reader stops

- **WHEN** a listing is superseded while its reader is still producing entries
- **THEN** the reader stops at its next check and sends no further entries, completion notice, or failure notice

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

### Requirement: Large directories stream progressively and stay responsive

The system SHALL show a directory's entries progressively while the directory is still being enumerated: the reader SHALL hand entries to the UI in batches bounded by an entry count and, while entries keep arriving, flushed at least about every 50 ms of enumeration time, so that the first entries and a growing `Reading… N` count appear before enumeration finishes, on local disks and slow network shares alike. Applying listing data SHALL NOT stall input handling or redrawing: each batch SHALL be merged into the sorted listing as a whole, in time proportional to the listing's size plus the cost of sorting the batch, never by inserting entries one at a time into the middle of the list; and the event loop SHALL apply at most about 8 ms or 64 messages of queued worker results before it processes pending input and redraws. Once the user has moved the cursor while a listing streams, arriving entries SHALL NOT move the cursor off the entry it is on, except where a same-directory re-read restores the cursor as "Same-directory re-read preserves cursor and scroll position" specifies. Name-based ordering and quick-filter matching SHALL use a lowercase name key computed once per entry when the entry is read, and the list of entries visible under the quick filter SHALL be recomputed only when the entries or the filter pattern change, so that, while no entries are selected, drawing a frame, hit-testing the mouse, and moving the cursor cost time proportional to the rows shown rather than to the directory's size.

#### Scenario: First entries appear before enumeration finishes

- **WHEN** a directory on a slow network share takes 20 seconds to enumerate and its entries arrive steadily throughout
- **THEN** entries and a growing `Reading… N` count are shown within about 50 ms of the first entries being read and keep growing while enumeration continues

#### Scenario: Input stays responsive while a 100,000-entry directory streams in

- **WHEN** a 100,000-entry directory is streaming in, many batches are already queued, and the user presses Down
- **THEN** the key is processed and the screen redrawn after at most about 8 ms or 64 queued worker messages, without waiting for the remaining batches to be applied

#### Scenario: Streaming 100,000 entries costs about one sort

- **WHEN** the release-build benchmark `bench_100k_ignored` streams 100,000 entries, in shuffled arrival order, into a panel in the reader's batch sizes
- **THEN** the total time spent applying the batches is at most three times the time of one bulk sort of the same 100,000 entries measured in the same run

#### Scenario: Cursor stays on its entry while entries stream in

- **WHEN** the user has moved the cursor to `report.txt` while the listing is still streaming, and a batch arrives containing entries that sort before `report.txt`
- **THEN** the cursor is still on `report.txt` after the batch is applied, and the entries remain in sorted order

#### Scenario: Frame cost does not grow with directory size

- **WHEN** the benchmark renders frames of a 100,000-entry panel and of a 1,000-entry panel with no entries selected, once with no quick filter and once with an active quick filter
- **THEN** a 100,000-entry frame takes no more than twice as long as a 1,000-entry frame under the same conditions

#### Scenario: Orders are unchanged by precomputed keys

- **WHEN** a listing is sorted by name, extension, time, or size, ascending or descending
- **THEN** the resulting order is identical to the case-insensitive orders and tie-breaks the panel produced before keys were precomputed, with `..` first
