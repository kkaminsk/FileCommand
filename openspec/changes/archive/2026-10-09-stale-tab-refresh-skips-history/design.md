## Context

`begin_listing` (`crates/filecommand-core/src/update.rs`) wraps `begin_listing_inner` with navigation bookkeeping: `quicksearch::record_visit(&mut state.dir_history, &path, state.clock_ms)` and an `Effect::PersistHistory` carrying the whole history file. Its own doc comment names the one existing bypass — tree mode's cursor-move preview, which calls `begin_listing_inner` directly because a preview "is the tree being browsed, not 'the user navigating the active panel into a directory'".

`refresh-inactive-panel-on-delete` made tab activation effectful: `SwitchTab`/`CloseTab` call `begin_listing` when the newly-active tab was marked stale. That choice followed the proposal's letter ("reuse the existing begin_listing/Effect::StartListing path") but inherited the navigation bookkeeping, which is semantically wrong for a refresh and observably wrong in two places: the Ctrl+J frecency ranking and an unnecessary `config.toml` write per activation.

## Goals / Non-Goals

**Goals:**
- The stale-tab activation refresh is invisible to fuzzy-jump history and to configuration persistence.
- Everything else about the refresh — `Effect::StartListing`, streaming chunks, Info-mode re-query, git-info re-query — stays identical to ordinary navigation.

**Non-Goals:**
- Changing `JobDone`'s immediate active-panel re-read (predates this change; also a refresh, but out of scope — it is the primary path and its history effects were accepted long ago; a broader "refreshes never record history" rule can be a separate proposal if desired).
- Changing what `begin_listing` itself does for genuine navigations.

## Decisions

- **Use the existing `begin_listing_inner` bypass rather than adding a flag parameter to `begin_listing`.** A `record_history: bool` parameter would thread a two-valued concern through every call site for exactly one false caller; the `begin_listing_inner` seam already exists, is already used for the identical non-navigation case, and keeps the call sites self-describing. Update the `begin_listing_inner` doc comment so it names both non-navigation callers.
- **Keep the refresh effects otherwise byte-identical.** `begin_listing_inner` already emits `Effect::StartListing`, the Info-mode query when applicable, and the git-info query — the only difference from `begin_listing` is the two navigation side effects, which is exactly the desired delta.

## Risks / Trade-offs

- [Risk] `begin_listing_inner`'s contract ("everything except navigation bookkeeping") could be diluted by future callers wanting other tweaks. → Mitigation: its doc comment now enumerates both non-navigation callers and the reason; a third caller should prompt extracting a purpose-named helper instead.
