## ADDED Requirements

### Requirement: Switching or closing a tab mid-listing never mixes listings

A tab's directory listing SHALL only ever add entries to that tab. When the active tab stops being active while its listing is still being read — by Alt+1..9 switching to another tab, or by Ctrl+W closing it — the system SHALL stop that read and SHALL NOT let any of its results reach the newly active tab. A tab switched away from in this state SHALL be marked stale and SHALL be refreshed with a fresh read when it next becomes active, through the same non-navigation refresh as "Stale background tab refresh on activation", instead of keeping or showing a partial listing. When the interrupted listing was a re-read of the tab's own directory, that refresh SHALL restore the cursor and scroll position the interrupted re-read was restoring, as panel-navigation "Same-directory re-read preserves cursor and scroll position" requires for a stale tab's activation refresh. When Ctrl+T opens a new tab while the active tab's listing is still being read, the listing SHALL continue to fill the new tab, which shows the same directory, and the original tab, now in the background, SHALL be marked stale.

#### Scenario: Switching tabs mid-listing

- **WHEN** tab 1 of the active panel is still streaming the listing of `C:\big` and the user presses Alt+2 to activate tab 2 at `C:\Work`
- **THEN** tab 2 shows only `C:\Work`'s entries, no entry of `C:\big` ever appears in it, and the read of `C:\big` is stopped

#### Scenario: Returning to an interrupted tab re-reads it

- **WHEN** the user later presses Alt+1 to return to the tab whose listing of `C:\big` was interrupted
- **THEN** the tab starts a fresh read of `C:\big` and shows `Reading… N` until that read completes, rather than a partial listing that never completes
- **AND** the refresh does not record a fuzzy-jump history visit or persist configuration

#### Scenario: An interrupted re-read still keeps the user's place

- **WHEN** the cursor is on `report.txt` partway down tab 1's complete listing, the user presses Ctrl+R, switches to tab 2 with Alt+2 before the re-read finishes, and later returns with Alt+1
- **THEN** the refreshed listing of tab 1 puts the cursor back on `report.txt` with the scroll offset it had before Ctrl+R

#### Scenario: Closing a tab mid-listing

- **WHEN** the active tab is still streaming a listing and the user presses Ctrl+W
- **THEN** that read is stopped and the neighboring tab that becomes active receives none of its entries

#### Scenario: New tab mid-listing keeps the listing

- **WHEN** the active tab is still streaming the listing of `C:\big` and the user presses Ctrl+T
- **THEN** the new tab, showing `C:\big`, keeps receiving the listing until it completes
- **AND** activating the original tab later re-reads `C:\big` instead of showing the partial listing it held when Ctrl+T was pressed

## MODIFIED Requirements

### Requirement: Stale background tab refresh on activation

A tab marked stale — by a completed file-operation job (see `file-operations` — "Automatic panel re-read on completion"), or by stopping being the active tab while its own listing was still being read (see "Switching or closing a tab mid-listing never mixes listings") — SHALL be refreshed with a fresh directory read the moment it becomes the active tab — via Alt+1..9 switch, or via the neighbor activation that follows a Ctrl+W close — instead of displaying its previously cached entries. The deferred refresh is not a navigation: it SHALL re-read the directory but SHALL NOT record a fuzzy-jump history visit for it or persist configuration. A tab that is not marked stale SHALL continue to activate from its cached state with no re-read, exactly as today.

#### Scenario: Switching to a stale background tab triggers a fresh read
- **WHEN** a background tab is browsing a directory affected by a completed job while it was inactive, and the user activates it with Alt+`n`
- **THEN** the tab re-reads its directory instead of showing its stale cached entries

#### Scenario: Closing a tab activates a neighbor that is stale
- **WHEN** Ctrl+W closes the active tab and falls back to an adjacent tab that was marked stale
- **THEN** the newly-active tab re-reads its directory instead of showing its stale cached entries

#### Scenario: Switching to a tab with no pending staleness is unchanged
- **WHEN** the user switches to a tab that was not affected by any completed job since it was last active, and whose listing had finished being read when it was left
- **THEN** the tab activates from its cached state with no re-read, as before

#### Scenario: The deferred refresh is not a navigation
- **WHEN** a stale background tab is activated and re-reads its directory
- **THEN** the directory is not added to or re-ranked in the fuzzy-jump history, and no configuration file is persisted as a result of the refresh
