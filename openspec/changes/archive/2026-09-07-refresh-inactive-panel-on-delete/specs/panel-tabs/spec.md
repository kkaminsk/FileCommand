## ADDED Requirements

### Requirement: Stale background tab refresh on activation

A tab marked stale by a completed file-operation job (see `file-operations` — "Automatic panel re-read on completion") SHALL be refreshed with a fresh directory read the moment it becomes the active tab — via Alt+1..9 switch, or via the neighbor activation that follows a Ctrl+W close — instead of displaying its previously cached entries. A tab that is not marked stale SHALL continue to activate from its cached state with no re-read, exactly as today.

#### Scenario: Switching to a stale background tab triggers a fresh read
- **WHEN** a background tab is browsing a directory affected by a completed job while it was inactive, and the user activates it with Alt+`n`
- **THEN** the tab re-reads its directory instead of showing its stale cached entries

#### Scenario: Closing a tab activates a neighbor that is stale
- **WHEN** Ctrl+W closes the active tab and falls back to an adjacent tab that was marked stale
- **THEN** the newly-active tab re-reads its directory instead of showing its stale cached entries

#### Scenario: Switching to a tab with no pending staleness is unchanged
- **WHEN** the user switches to a tab that was not affected by any completed job since it was last active
- **THEN** the tab activates from its cached state with no re-read, as before
