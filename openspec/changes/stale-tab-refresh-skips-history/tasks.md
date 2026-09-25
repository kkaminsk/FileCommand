## 1. Core: route the stale-activation refresh past navigation bookkeeping

- [x] 1.1 In `crates/filecommand-core/src/update.rs`, change the `Command::SwitchTab` and `Command::CloseTab` stale-activation arms from `begin_listing` to `begin_listing_inner`.
- [x] 1.2 Update `begin_listing_inner`'s doc comment so it names both non-navigation callers (tree preview, stale-tab refresh) and the shared rationale.
- [x] 1.3 `JobDone`'s immediate re-read stays on `begin_listing` — no change.

## 2. Tests

- [x] 2.1 Extend the `refresh-inactive-panel-on-delete` tests: activating a stale tab produces no `Effect::PersistHistory` and leaves `state.dir_history` unchanged.
- [x] 2.2 Same assertion for the Ctrl+W neighbor-fallback refresh.
- [x] 2.3 Confirm a genuine navigation (e.g. Enter on a subdirectory) still records the visit and persists history — guarding against over-broad bypass.

## 3. Verification

- [x] 3.1 Run `cargo test -p filecommand-core` and `cargo test -p filecommand-tui`.
- [x] 3.2 `openspec validate --all`.
- [x] 3.3 `detect_changes()` (GitNexus) against `main` to confirm only the expected symbols/flows are touched before opening the PR.
