## 1. Wire the Delete key

- [x] 1.1 In `crates/filecommand-tui/src/input/mod.rs`, add a `KeyCode::Delete if is_plain(&key) => Some(Command::RequestDelete)` arm to `map_panel_key`, placed among the unmodified keys near the existing `KeyCode::F(8)` arm.

## 2. Regression test

- [x] 2.1 In `crates/filecommand-tui/src/input/tests.rs`, add an assertion that `plain(KeyCode::Delete)` maps to `Some(Command::RequestDelete)`, parallel to the existing F8 assertion.
- [x] 2.2 (Optional) Assert that a modified Delete (e.g. Ctrl+Delete) does **not** map to `RequestDelete`, locking in the plain-only guard.

## 3. Verify

- [x] 3.1 Run `cargo test -p filecommand-tui` (and the workspace test suite) — all green.
- [ ] 3.2 Run `openspec validate delete-key-triggers-delete --strict` — clean.
- [ ] 3.3 Manually confirm in the running app: cursor on a file, press Delete → the delete-confirmation dialog appears; accept it → the file is deleted; press Delete then Esc → nothing is deleted.
