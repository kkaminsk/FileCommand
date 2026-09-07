## 1. Core: stale-tab tracking

- [x] 1.1 Add `stale: bool` (default `false`) to `TabData` in `crates/filecommand-core/src/panel.rs`; thread it through `to_tab_data`/`adopt_tab_data`.
- [x] 1.2 Add a `PanelState` method to mark every background tab (i.e. every entry in `self.tabs`) whose `cwd` matches a given directory as stale.
- [x] 1.3 Add a way to read and clear the active tab's incoming staleness when it becomes active (used by `switch_tab`/`close_tab`'s activation step).

## 2. Core: extend job-completion re-read to background tabs

- [x] 2.1 In `update.rs`'s `JobDone` handling, alongside the existing `panels_matching`-driven active-panel re-read, mark stale any background tab on either side whose directory matches `source_dir`/`dest_dir`.
- [x] 2.2 Unit test: `JobDone` for a Delete marks a background tab on the same directory stale without eagerly re-reading it (no `Effect::StartListing` for that tab).
- [x] 2.3 Unit test: existing active-panel re-read behavior (both `job_done_with_no_skips_rereads_matching_panels_and_returns_to_panels` and `job_done_only_rereads_panels_whose_cwd_matches_source_or_dest`) still passes unchanged.

## 3. Core: refresh on tab activation

- [x] 3.1 Update `Command::SwitchTab`/`Command::CloseTab` (and `Command::OpenTab` if it can activate a stale inherited tab) handlers in `update.rs` to check the newly-active tab's stale flag after calling into `panel.rs`, and — if set — clear it and push the same `begin_listing` effects used elsewhere for a fresh read.
- [x] 3.2 Unit test: switching (Alt+`n`) to a tab previously marked stale issues `Effect::StartListing` for that panel and clears the stale flag.
- [x] 3.3 Unit test: closing the active tab (Ctrl+W) and falling back to a stale neighbor issues a fresh read.
- [x] 3.4 Unit test: switching to a non-stale tab still issues no listing effects (unchanged behavior).

## 4. Verification

- [x] 4.1 Add/extend a same-directory dual-panel test: both panels' active tabs on the same directory, delete completes, both re-read immediately (covers the base case already confirmed working, guards against regression).
- [x] 4.2 Run `cargo test -p filecommand-core` and `cargo test -p filecommand-tui`.
- [ ] 4.3 Manual check via the `run` skill: two tabs on one panel, background tab pointed at the same folder the active tab (or the opposite panel) deletes from; switch to the background tab and confirm the deleted entry is gone.
- [ ] 4.4 `detect_changes()` (GitNexus) against `main` to confirm only the expected symbols/flows are touched before opening the PR.
