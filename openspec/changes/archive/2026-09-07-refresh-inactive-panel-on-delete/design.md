## Context

Linear BIG-162 reports: delete a file in one pane, and the other pane — browsing the same folder — keeps showing stale contents.

Investigation of `crates/filecommand-core::update`:

- `Command::JobDone` already calls `panels_matching(state, &[&source_dir, &dest_dir])`, which checks **both** `PanelSide::Left` and `PanelSide::Right`'s *currently active* directory against the job's affected paths and re-reads any match (`fs_ops/job.rs` deliberately sets `dest_dir == source_dir` for `Delete`/`Rename` "to keep the type uniform for panel-re-read matching"). A test added and run during investigation (`JobDone` with both panels' active cwd set to the same deleted-from directory) confirms both panels' `Effect::StartListing` fire correctly today — the plain, no-tabs case already works.
- `PanelState` (panel-tabs, `crates/filecommand-core/src/panel.rs`) holds `tabs: Vec<TabData>` plus the active tab's state inline. Each `TabData` snapshots its own `entries: Vec<Entry>` when it stops being active. `switch_tab`/`close_tab`/`open_tab` (`panel.rs`) swap `TabData` in and out via `apply_tab_list`/`adopt_tab_data` with **no re-listing** — confirmed by their `core::update` call sites (`Command::SwitchTab`/`OpenTab`/`CloseTab`), which mutate state and push no effects.

So the real gap is background tabs: a tab that isn't the currently-displayed one for its panel side keeps a private cached listing that a completed job never touches, and switching to it later shows exactly what was cached — including now-deleted entries — until something else forces a re-read.

## Goals / Non-Goals

**Goals:**
- Any tab (active or background, either panel side) whose directory is affected by a completed/cancelled-with-partial-changes job eventually shows correct contents without a manual refresh.
- Preserve the existing immediate-refresh behavior for active tabs exactly as it works today.
- Avoid unnecessary I/O: don't eagerly re-read directories the user isn't currently looking at.

**Non-Goals:**
- Re-architecting the tab data model or the streaming-listing mechanism itself.
- Refreshing tabs on other, unaffected directories.
- Cross-process file-system change notification (e.g. watching for changes made outside the app) — this only covers jobs FileCommand itself runs.

## Decisions

- **Mark-stale-and-refresh-on-activate for background tabs, not eager re-read.** Add a `stale: bool` field to `TabData` (default `false`). On `JobDone`, in addition to the existing active-tab re-read, walk `tabs` on both panels and set `stale = true` on any whose `cwd` matches an affected path. `switch_tab`, and the tab activation `close_tab` performs when falling back to a neighbor, check the newly-active tab's `stale` flag; if set, clear it and have `core::update` push the existing `begin_listing` effects for that panel instead of the plain no-op it does today.
  - Alternative considered: eagerly re-read every matching tab (active or not) the moment the job completes. Rejected — a panel can have several background tabs, and spawning a listing thread per stale tab for directories the user may never revisit wastes I/O and worker threads for no visible benefit; deferring to activation-time gets the same correctness with none of the waste.
- **Reuse the existing `begin_listing`/`Effect::StartListing` path for the deferred refresh**, rather than inventing a second listing mechanism, so behavior (streaming chunks, git-info re-query, Info-mode re-query) stays identical between "just-activated tab happens to need a read" and "normal navigation."
- **Staleness is directory-keyed, not job-keyed.** A tab is marked stale by directory match only (same comparison `panels_matching` already does), with no dependency on which job or job kind caused it — simpler, and matches the existing requirement's job-kind-agnostic wording ("copy/move/delete/mkdir job completes").

## Risks / Trade-offs

- [Risk] `switch_tab`/`close_tab` currently return no effects (pure state mutation); making tab activation effectful is a small API shape change for their `core::update` call sites. → Mitigation: only `Command::SwitchTab`/`OpenTab`/`CloseTab` handlers in `update.rs` change to collect and return effects (as most other commands already do); `panel.rs`'s tab methods stay effect-free and simply expose whether the newly-active tab was stale.
- [Risk] A directory could be deleted entirely (not just a file within it) while cached in a background tab; re-reading on activation must surface the existing "directory no longer exists" handling rather than a new failure mode. → Mitigation: the deferred refresh reuses `begin_listing`, so it goes through the same `ListingFailed` path already exercised by ordinary navigation into a since-removed directory (see `filesystem-error-handling`).
- [Trade-off] Between the job completing and the user later switching to the stale tab, its cached entries remain visibly wrong if the user opens the tab strip preview or otherwise glances at background-tab state outside of activating it. Acceptable: today's UI only shows a background tab's directory label in the tab strip, not its entries, so nothing currently surfaces stale content without activation.
