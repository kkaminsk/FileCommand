## Why

`refresh-inactive-panel-on-delete` reused `begin_listing` for the deferred stale-tab refresh, and `begin_listing` does two things beyond starting the read: it records a fuzzy-jump frecency visit for the directory and pushes `Effect::PersistHistory` (a `config.toml` disk write). A background-consistency refresh is not a user navigation — the user pressed Alt+`n` or Ctrl+W to change tabs, not to visit the directory — so the side effects are wrong in kind:

- The directory the tab happens to sit on gets its fuzzy-jump frecency rank bumped every time a job touches it and the tab is later activated, inflating directories the user never deliberately visited toward the top of the Ctrl+J list.
- Every stale-tab activation writes `config.toml` to disk for a state change the user never made.

The codebase already has the seam for exactly this distinction: tree mode's cursor-move preview of the opposite panel calls `begin_listing_inner` directly precisely because that preview "is the tree being browsed, not 'the user navigating the active panel into a directory'" (the comment on `begin_listing` in `update.rs`). The deferred refresh is the same shape of non-navigation.

## What Changes

- `Command::SwitchTab` / `Command::CloseTab`'s stale-activation refresh issues the read via `begin_listing_inner` instead of `begin_listing`: same `Effect::StartListing`, same Info-mode re-query, same git-info re-query — but no fuzzy-jump history visit and no `Effect::PersistHistory`.
- The immediate active-panel re-read on `JobDone` is unchanged (that one *is* a real refresh of what the user is looking at, and it predates this change).
- Navigation-driven tab switches were never re-reads; unchanged.

## Capabilities

### New Capabilities
(none)

### Modified Capabilities
- `panel-tabs`: narrows "Stale background tab refresh on activation" so the deferred refresh is explicitly not a navigation — it SHALL NOT record a fuzzy-jump history visit or persist configuration.

## Impact

- `crates/filecommand-core/src/update.rs`: the two `begin_listing` calls in the `SwitchTab`/`CloseTab` arms become `begin_listing_inner`; the existing `begin_listing_inner` doc comment gains the stale-refresh case alongside the tree-preview case.
- Existing tests from `refresh-inactive-panel-on-delete` keep passing (they assert `Effect::StartListing` and the cleared flag, never history/persist effects); new tests pin the absence of `Effect::PersistHistory` and of a frecency visit.
