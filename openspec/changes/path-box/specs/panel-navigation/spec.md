## MODIFIED Requirements

### Requirement: Parent-directory navigation

The system SHALL navigate the active panel to its parent directory when Ctrl+PgUp is pressed, and when Backspace is pressed while no path box has focus. When the current directory is already a filesystem root with no parent, the navigation SHALL be a no-op.

#### Scenario: Ctrl+PgUp goes to the parent

- **WHEN** the active panel is in a non-root directory and Ctrl+PgUp is pressed
- **THEN** the panel SHALL navigate to its parent directory and begin listing it

#### Scenario: Backspace on empty command line goes to the parent

- **WHEN** no path box has focus and Backspace is pressed
- **THEN** the active panel SHALL navigate to its parent directory

#### Scenario: Parent navigation at a root is a no-op

- **WHEN** the active panel is at a filesystem root with no parent and a parent-navigation command is issued
- **THEN** the panel SHALL remain in the current directory unchanged

### Requirement: Swap panels (Ctrl+U)

The system SHALL exchange the left and right panels' complete state — directory, tabs, listing, cursor, scroll offset, selection, sort mode, filter, display mode, and git info — on Ctrl+U or Commands → Swap panels, keeping the active side and the split percentage unchanged, without re-reading either directory. A swap requested while either panel's listing is still streaming SHALL be ignored. After a swap the system SHALL re-issue each panel's pending Info-mode and git-info queries so no reply from before the swap is applied to the wrong panel.

#### Scenario: Swap keeps focus on the same side
- **WHEN** the left panel is active showing `C:\a` and the right shows `D:\b`, and the user presses Ctrl+U
- **THEN** the left panel shows `D:\b`, the right shows `C:\a`, the left panel is still active, and the left path box reads `D:\b\` while the right reads `C:\a\`

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

- **WHEN** the active panel enters `C:\big` and, while that listing is still streaming, the user presses Backspace to return to `C:\`
- **THEN** the panel shows only the entries of `C:\` — no entry from `C:\big`, not even `C:\big`'s `..` entry — and the `Reading… N` count covers only `C:\`'s entries
- **AND** the read of `C:\big` is signalled to stop

#### Scenario: cd elsewhere mid-listing

- **WHEN** the panel's listing of `D:\logs` is still streaming and the user focuses the path box, types `C:\Work\notes.txt` (an existing file) and presses Enter, so the panel navigates to `C:\Work`
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
