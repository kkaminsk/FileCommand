## 1. Core: configuration

- [ ] 1.1 Add `refresh_interval: u32` (seconds, default 60) to `Config` in `crates/filecommand-core/src/config.rs`; parse it as a non-negative integer next to `panel_split`; an uninterpretable value yields a line-numbered warning and the default (configuration-files "Malformed or unrecognized configuration lines produce startup warnings").
- [ ] 1.2 Thread it into `State` as `refresh_interval_ms` (`0` = disabled); `State::empty` disables it so existing reducer and snapshot tests are unaffected.
- [ ] 1.3 Unit tests: default, `0`, a positive value, and the `refresh_interval = fast` / `-5` warnings.

## 2. Core: content digest

- [ ] 2.1 Add `listing::content_digest(entries)` hashing `(name, kind, size, modified, hidden, link)` over entries sorted by name, plus the count; the synthetic `..` is excluded.
- [ ] 2.2 Add `listing::read_dir_digest(reader, path, cancel) -> io::Result<u64>` — a non-streaming enumeration through the same `classify_entry` path, skipping unreadable entries exactly as the listing does.
- [ ] 2.3 `PanelState::content_digest: Option<u64>` — computed at `ListingComplete`, cleared by `begin_new_listing`; tests that identical entry sets in different arrival order digest equally and that a change to any hashed field differs.

## 3. Core: probe scheduling and application

- [ ] 3.1 Add `Command::DirectoryProbed { panel, path, request, digest: Option<u64> }` and `Effect::ProbeDirectory { panel, path, request }` in `update/command.rs`.
- [ ] 3.2 Add `State::next_probe_ms` and `PanelState::probe_request`; on `Tick`, when due and `refresh_interval_ms > 0`, issue a probe for each eligible panel (design D3) and advance `next_probe_ms`.
- [ ] 3.3 Extend `anim::next_wake_ms` with the probe deadline (only when enabled and some panel is eligible).
- [ ] 3.4 Extract `reread_in_place(state, side)` in `update/listing.rs` from `refresh_activated_stale_tab`; make the latter call it; update `begin_listing_inner`'s doc comment.
- [ ] 3.5 Apply `DirectoryProbed`: drop on request/path mismatch or ineligibility; `None` or an equal digest → clear `probe_request`, no effects; a differing digest → `reread_in_place`.
- [ ] 3.6 Reducer tests: a probe is issued once per interval for both eligible panels; not issued while an overlay, viewer, editor, file-op dialog/job, drag, or type-ahead is active, for a streaming or errored panel, or for Info/Tree/QuickView; one outstanding probe per panel; a result is dropped when a dialog opened meanwhile; a no-change result yields no effects; a changed result starts a listing with the anchor captured and emits no `PersistHistory`; `refresh_interval_ms == 0` never probes; `next_wake_ms` reports the deadline.

## 4. Core: F12 slot command

- [ ] 4.1 `keybar_command(12)` → `Command::RereadPanel(active)` in `update/overlays.rs` (resolve the active side at the call site or pass it in), with a test.

## 5. TUI: key, bar, click

- [ ] 5.1 `KeyCode::F(12)` → `Command::RereadPanel(active)` in `input/mod.rs`'s panel key map; test alongside the Ctrl+R test in `input/tests.rs`.
- [ ] 5.2 `views/keybar.rs`: add `("12", "Refresh")`, `("12", "Rfr")`, `("12", "")` to the three forms; update the width tests to 77/56/23; `hit_slots` yields slot 12; the drag relabel is unchanged.
- [ ] 5.3 Mouse test: a click on the `12Refresh` rect dispatches `KeybarPress(12)` and the reducer turns it into a re-read of the active panel.
- [ ] 5.4 Regenerate the `tests/snapshot_matrix.rs` key-bar snapshots and any panel snapshots that include the bar; confirm 60 columns renders `… 10Qit 12Rfr` in full and 66 columns still renders the short form.

## 6. TUI: worker and event loop

- [ ] 6.1 `worker::spawn_probe(panel, path, request, cancel, tx, token)` runs `read_dir_digest` and delivers `DirectoryProbed` (`digest: None` on error); it never delivers after cancellation.
- [ ] 6.2 `app.rs` runs `Effect::ProbeDirectory`; a `DirectoryProbed` that yields no effects does not set the redraw flag (design D5).
- [ ] 6.3 Integration test (`tests/integration_smoke.rs`): with a 1-second interval, create a file externally → the panel lists it after the next probe with the cursor on its previous entry; with nothing changed, no frame is drawn across a probe.

## 7. Docs

- [ ] 7.1 `README.md`: key table row `Ctrl+R / F12 | Re-read (refresh) the active panel`; the configuration section documents `refresh_interval` (seconds, default 60, `0` off) and the automatic refresh.
- [ ] 7.2 The Help "Keyboard reference" topic text mentions F12 next to Ctrl+R.

## 8. Verification

- [ ] 8.1 `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` all pass.
- [ ] 8.2 Manual: run at 80×24 and 60×16 and check the bar; press F12 and click `12Refresh`; set `refresh_interval = 5`, touch a file from another shell, see it appear with the cursor in place; open F8 on a file, delete it externally, wait a probe — the dialog's target does not change.
