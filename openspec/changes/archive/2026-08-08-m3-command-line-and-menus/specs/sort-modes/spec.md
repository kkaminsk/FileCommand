## ADDED Requirements

### Requirement: Sort-mode keybindings

The active panel SHALL support five sort modes selectable by keyboard: Ctrl+F3 sorts by Name, Ctrl+F4 by Extension, Ctrl+F5 by Time, Ctrl+F6 by Size, and Ctrl+F7 sets Unsorted order. Each keystroke MUST set the active panel's sort mode and re-sort its already-gathered entry list in place, without re-reading the directory or re-`stat`-ing entries.

#### Scenario: Ctrl+F3 sorts by name

- **WHEN** the active panel is in an arbitrary sort mode and Ctrl+F3 is pressed
- **THEN** the panel's sort mode becomes Name and the entry rows are re-ordered by name using the property-tested Name comparator

#### Scenario: Ctrl+F4 sorts by extension

- **WHEN** the active panel is focused and Ctrl+F4 is pressed
- **THEN** the panel's sort mode becomes Extension and the entry rows are re-ordered by extension

#### Scenario: Ctrl+F5 and Ctrl+F6 sort by time and size

- **WHEN** Ctrl+F5 (Time) then Ctrl+F6 (Size) are pressed
- **THEN** the panel's sort mode becomes Time and then Size, and after each keystroke the entry rows are re-ordered by the corresponding comparator

#### Scenario: Ctrl+F7 restores unsorted order

- **WHEN** the active panel is in a sorted mode and Ctrl+F7 is pressed
- **THEN** the panel's sort mode becomes Unsorted and entries render in directory-enumeration order

#### Scenario: Sort operates without re-reading the directory

- **WHEN** any sort-mode key is pressed
- **THEN** the panel re-sorts the entries already held in memory and issues no new directory read and no per-entry metadata query

### Requirement: Stable sort applied to gathered entries

Sorting SHALL be stable and applied to the entry metadata already gathered from the directory enumeration. The sort MUST be independent per panel, so changing one panel's sort mode does not affect the other panel's sort mode or ordering.

#### Scenario: Sort is stable for equal keys

- **WHEN** two entries compare equal under the active comparator
- **THEN** their relative order after sorting matches their relative order before sorting

#### Scenario: Sort mode is per-panel

- **WHEN** the left panel is set to Size and the right panel is set to Name
- **THEN** each panel retains its own sort mode and ordering independently

### Requirement: Header sort-column arrow indicator

The active panel's column-header row SHALL display a `↓` (ascending) or `↑` (descending) arrow next to the label of the column that is the current sort key, styled with the `panel.header` role. When the sort mode is Unsorted, no sort arrow SHALL be shown on any column.

#### Scenario: Arrow marks the sorted column

- **WHEN** the panel sort mode is Name
- **THEN** the header renders the sort arrow adjacent to the Name column label (e.g. `C:↓ Name`) and no arrow on the Size, Date, or Time columns

#### Scenario: Arrow moves when the sort key changes

- **WHEN** the sort mode changes from Name to Size
- **THEN** the arrow indicator disappears from the Name column and appears next to the Size column label

#### Scenario: No arrow in unsorted mode

- **WHEN** the panel sort mode is Unsorted
- **THEN** the header row shows no `↓`/`↑` arrow on any column

### Requirement: Re-read the panel

Ctrl+R SHALL re-read the active panel's directory, replacing its entry list with a fresh streaming read while preserving the panel's current sort mode. The re-read MUST use the same streaming read path as the initial listing, so a large directory fills progressively and shows `Reading… N` in the mini-status until the read completes.

#### Scenario: Ctrl+R re-reads the directory

- **WHEN** Ctrl+R is pressed on a focused panel
- **THEN** the panel discards its current entries and begins a fresh streaming read of the same directory

#### Scenario: Re-read preserves sort mode

- **WHEN** the panel sort mode is Size and Ctrl+R is pressed
- **THEN** the freshly read entries are sorted by Size and the header still shows the sort arrow on the Size column

#### Scenario: Re-read of a large directory streams with a running count

- **WHEN** Ctrl+R is pressed on a directory large enough to stream in chunks
- **THEN** the mini-status shows `Reading… N` with N updating as chunks arrive, reverting to the normal mini-status display when the read completes
