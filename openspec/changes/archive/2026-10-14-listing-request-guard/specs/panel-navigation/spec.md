## ADDED Requirements

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
