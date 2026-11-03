## Why

A panel only re-reads after FileCommand's own operations, on Ctrl+R or the Left/Right menu's Re-read item, or when a stale tab is activated. Anything another program does to the directory on screen — a compiler writing output, a download finishing, a file deleted from another shell — is invisible until the user remembers to press Ctrl+R, a chord that is not shown anywhere on screen. The design doc deferred filesystem watching to v2 and made Ctrl+R the only manual refresh (design doc, "Refresh policy"). This change adds a cheap periodic check that re-reads a panel only when its directory actually changed, and puts a visible `12Refresh` slot on the F-key bar so manual refresh is discoverable.

## What Changes

- Every `refresh_interval` seconds (default 60) a worker thread enumerates each eligible panel's current directory and reports a content digest of its direct entries (name, kind, size, modified time, hidden/system flag, link flag). The reducer compares it with the digest of the listing on screen: identical means nothing happens — no state change, no redraw; different means the panel re-reads in place.
- The automatic re-read is the same in-place refresh a stale tab gets on activation: cursor and scroll position are restored, the selection (keyed by entry identity) survives, the sort mode is kept, git info is re-queried, and no fuzzy-jump history visit or configuration write results.
- A probe is only issued — and its result only applied — while the panels are the active screen with no dialog, menu, drag, or type-ahead in progress, and only for a panel whose listing is complete, error-free, and shown in a list display mode (Full or Brief). A probe whose directory cannot be read changes nothing. At most one probe per panel is outstanding at a time.
- `refresh_interval` is a new top-level `config.toml` setting: a non-negative integer number of seconds; `0` turns automatic refresh off; a value that cannot be interpreted produces the standard startup warning and the default (60) applies.
- **F12** re-reads the active panel exactly as Ctrl+R does. The main F-key bar gains an eleventh slot: full form `… 10Quit 12Refresh` (77 columns), short form `… 10Qit 12Rfr` (56 columns), numbers-only `… 10 12` (23 columns). A click on the new slot dispatches the same re-read. Slot 11 is not drawn (F11 stays unassigned). The viewer and editor F-key bars are unchanged.
- The idle event loop treats the next probe as a fourth time-driven wake, and a probe that finds no change does not redraw the screen.
- Non-goals: a filesystem watcher (`notify` / `ReadDirectoryChangesW`); scanning background tabs (they keep the stale-on-activation path); refreshing Tree, Info, or QuickView panels; a launch flag for the interval; any change to Ctrl+R's own behavior.

## Capabilities

### New Capabilities

- `auto-refresh`: periodic directory change detection, the in-place refresh it triggers, the conditions under which it stays out of the way, and the `refresh_interval` setting.

### Modified Capabilities

- `responsive-layout`: MODIFIED "F-key bar degradation forms" — the main bar has eleven slots; new form strings and widths.
- `sort-modes`: MODIFIED "Re-read the panel" — F12 is a second binding for the same re-read.
- `mouse-input`: MODIFIED "Key bar, menu bar, pull-down items, and dialog buttons are clickable" — adds the `12Refresh` click scenario.
- `application-shell`: MODIFIED "Idle event loop does not redraw" — the refresh probe is a time-driven wake; a no-change probe result does not redraw.
- `panel-navigation`: MODIFIED "Same-directory re-read preserves cursor and scroll position" — an automatic refresh is one of the re-reads that restore the user's place.
- `configuration-files`: MODIFIED "Malformed or unrecognized configuration lines produce startup warnings" — `refresh_interval` joins the keys whose uninterpretable value is reported.

## Impact

- **Core (`crates/filecommand-core`):**
  - `update/command.rs` — `Command::DirectoryProbed { panel, path, request, digest: Option<u64> }`; `Effect::ProbeDirectory { panel, path, request }`.
  - `update.rs` / `anim.rs` — `State::refresh_interval_ms`, `State::next_probe_ms`; `Tick` issues due probes; `anim::next_wake_ms` gains the probe deadline.
  - `update/listing.rs` — extract `reread_in_place(state, side)` from `refresh_activated_stale_tab` (the "third caller" `begin_listing_inner`'s doc comment anticipates) and use it for the automatic refresh; apply `DirectoryProbed`.
  - `update/overlays.rs` — `keybar_command(12)`.
  - `panel.rs` — `PanelState::probe_request: Option<u64>`, `PanelState::content_digest: Option<u64>` (set at `ListingComplete`, cleared by `begin_new_listing`).
  - `listing.rs` — `content_digest(entries)` shared by the listing and the probe; a non-streaming `read_dir_digest` for the worker.
  - `config.rs` — `refresh_interval` key, default 60, integer parsing with warning.
  - Tests: `update/tests.rs`, `anim.rs`, `panel.rs`, `listing.rs`, `config.rs`.
- **TUI (`crates/filecommand-tui`):**
  - `input/mod.rs` — `KeyCode::F(12)` → `Command::RereadPanel(active)` in the panel key map.
  - `views/keybar.rs` — eleven-slot forms, widths 77/56/23, `hit_slots` slot 12; `tests/snapshot_matrix.rs` key-bar snapshots regenerate.
  - `worker.rs` — `spawn_probe` (enumerate, digest, deliver `DirectoryProbed`).
  - `app.rs` — run `Effect::ProbeDirectory`; a `DirectoryProbed` that produces no effects does not set the redraw flag.
  - Tests: `input/tests.rs`, `input/mouse/tests.rs`, `tests/snapshot_matrix.rs`, `tests/integration_smoke.rs`.
- **Docs:** `README.md` key table gains `F12` next to Ctrl+R and documents the `refresh_interval` setting; the Help "Keyboard reference" topic text mentions F12.
- **No persisted-format or dependency changes.** `config.toml` gains one optional key.
