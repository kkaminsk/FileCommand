## MODIFIED Requirements

### Requirement: Re-read the panel

Ctrl+R and F12 SHALL each re-read the active panel's directory, replacing its entry list with a fresh streaming read while preserving the panel's current sort mode. The two keys SHALL dispatch the identical command, so everything specified for Ctrl+R anywhere holds for F12. The re-read MUST use the same streaming read path as the initial listing, so a large directory fills progressively and shows `Reading… N` in the mini-status until the read completes.

#### Scenario: Ctrl+R re-reads the directory

- **WHEN** Ctrl+R is pressed on a focused panel
- **THEN** the panel discards its current entries and begins a fresh streaming read of the same directory

#### Scenario: F12 re-reads the directory

- **WHEN** F12 is pressed on a focused panel
- **THEN** the panel re-reads exactly as it does for Ctrl+R, keeping the cursor's entry and scroll offset

#### Scenario: Re-read preserves sort mode

- **WHEN** the panel sort mode is Size and Ctrl+R is pressed
- **THEN** the freshly read entries are sorted by Size and the header still shows the sort arrow on the Size column

#### Scenario: Re-read of a large directory streams with a running count

- **WHEN** Ctrl+R is pressed on a directory large enough to stream in chunks
- **THEN** the mini-status shows `Reading… N` with N updating as chunks arrive, reverting to the normal mini-status display when the read completes
