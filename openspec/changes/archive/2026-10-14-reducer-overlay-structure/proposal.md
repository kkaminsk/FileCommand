# Change: reducer-overlay-structure

## Why

Two audit findings describe the same structural weakness. Which modal surface owns the screen and the input is encoded several times over, and nothing keeps the copies in step.

- **B-02 (Medium): one large reducer whose input-routing policy lives only in its control flow.**
  - `crates/filecommand-core/src/update.rs` is 3,707 lines (3,519 non-blank). It is built around `update_impl` (`update.rs:1063-1600`, 538 lines): a chain of early returns, one per surface.
  - Ten modal overlays are separate `Option` fields on `State`: `menu`, `drive_select`, `fuzzy_jump`, `find_file`, `user_menu`, `history_dialog`, `theme_picker`, `file_action_menu`, `help`, `startup_warning`. Each is gated in `update_impl` by `state.X.is_some() && is_X_command(&cmd)`, and its handler then re-`unwrap()`s the `Option` it was gated on. That makes 25 such unwraps in `update.rs`, safe only because of a gate hundreds of lines away. (Line references refreshed 2026-10-12 against branch `Spec`; `history_dialog` exists only on the verified branch `build/open-proposals-2026-10` — see design.md's refresh note.)
  - The overlay precedence is written down four separate times, in four different orders:
    - the reducer's gate chain (`update.rs:1230-1277`);
    - the key router (`crates/filecommand-tui/src/input/mod.rs:35-76`);
    - the mouse context (`input/mouse.rs:156-187`);
    - the render z-order (`views/mod.rs` `render_phase` and `render`).

    The orders differ. For example, the reducer checks the startup warning last of all the overlays, while the key router checks it first. The disagreement goes unnoticed only because the open paths happen never to leave two overlays open at once. Two key-router tests build such impossible states on purpose, to pin an order.
  - Adding a modal means touching at least four places. The audit attributes two of its own bugs, R-03 (Critical) and R-04 (High), to precedence and ordering omissions that are hard to see in a 538-line function.
  - The tests mirror the layout: `update/tests.rs` is 5,118 lines.
- **B-08 (Low): click targets are computed separately from what is drawn.**
  - Each dialog's renderer and its `hit_buttons` recompute the same geometry independently:
    - `views/delete_confirm.rs`: render at `:32`, hit at `:70`, including rebuilding the question text;
    - `views/quit_dialog.rs`: `:21` and `:44`;
    - `views/progress_dialog.rs`: `:33` and `:84`.
  - `views/mod.rs` `build_hitmap` (`:117-169`) re-derives "what is open" separately from `render_phase`.
  - The only guard is comments such as "mirrors the renderer's own geometry exactly". When one side changes, clicks silently land on the wrong cells.

Neither finding is a bug today. Both are the reason the next bug in this area is likely, and hard to see.

## What Changes

- **One overlay slot.** The ten `Option` fields become one `State::overlay: Option<Overlay>`, an enum with one variant per overlay. At most one overlay can be open by construction.
  - The quit-confirmation dialog stays a separate layer above everything. The existing spec lets it open over any context, and cancelling it restores that context exactly.
  - The file-operation dialogs, viewer, and editor stay phases of `UiPhase`, which are already exclusive.
- **One precedence order, in one place.** A core function names which layer owns input. The reducer's routing, the TUI's key router and mouse context, and the render and hit-map dispatch all consult it, instead of each encoding its own order. The single order is the reducer's current one:
  1. quit confirmation;
  2. file-operation dialog;
  3. viewer;
  4. editor;
  5. the overlay;
  6. panels or splash.

  `resize-preserves-session-state` will add an undersized layer directly below the quit confirmation, through which only the startup warning's keys also pass.

  A layer handles only the commands it claims. Everything else falls through to the next layer down, as today, so worker replies such as job progress still reach a running job while the quit dialog is open.
- **Handlers receive their state directly.** Each overlay's handler is given its own state (`&mut FuzzyJumpState`, and so on) taken from the slot, and it returns whether the overlay stays open, closes, or is replaced. The nine `is_*_command` predicates collapse into one `Overlay::claims(&Command)`, and the 25 unwraps disappear.
- **Reducer split.** `update.rs` is split into `update/{listing,file_op,viewer,editor,overlays,panels}.rs`, with `Command` and `Effect` in `update/command.rs`. Public paths are unchanged through re-exports. `update/tests.rs` is split along the same lines.
- **One layout per dialog.** Each dialog and menu with clickable parts exposes one layout function, returning its rectangle and button or row rectangles. Its renderer and its hit test both use it, and the hit map is filled in the same dispatch that draws.
- **No user-visible behavior change.** Rendering, key handling, and click handling stay the same for every reachable state. The insta snapshots are the safety net and are expected not to change.
- **Incremental.** One overlay per commit, with a debug assertion during the transition that at most one overlay is set.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `application-shell`: adds the requirement "Modal overlays are mutually exclusive by construction". It covers the single slot, overlay-to-overlay replacement exactly as today, unbound keys being ignored while an overlay is open, stale replies never touching another overlay, and one precedence order.
- `mouse-input`: adds the requirement "Click targets match the rendered geometry". Every dialog's and menu's clickable cells are exactly the cells where its buttons or rows are drawn, at every size and after a resize.

## Impact

- **`crates/filecommand-core/src/update.rs`:**
  - `State`: ten overlay fields are replaced by `overlay: Option<Overlay>`, plus accessors.
  - `update_impl`: the gate chain becomes one routing step.
  - `drag_allowed` becomes `state.overlay.is_none()`, plus the existing phase and quit checks.
  - Every `is_*_command` and `handle_*` overlay function (`handle_drive_select`, `handle_menu`, `handle_fuzzy_jump`, `handle_find_file`, `handle_user_menu`, `handle_history_dialog`, `handle_theme_picker`, `handle_file_action_menu`, `handle_help`) and every opener in the `Panels` arm (`FuzzyJumpOpen`, `FindFileOpen`, `UserMenuOpen`, `HistoryDialogOpen`, `ThemePickerOpen`, `HelpOpen`, `MenuOpen`, `MenuTitleClick`, `DriveListReady`, `handle_enter`, `handle_open_action_menu_at`, `apply_user_menu_reload`).
  - The module is split into `update/*.rs`. `update/tests.rs` is split into `update/tests/*.rs`.
- **`crates/filecommand-core/src/stamps.rs`:** `DialogMark` and the menu and popup marks read the slot instead of the separate fields.
- **`crates/filecommand-core/src/lib.rs`:** re-exports `Overlay`. Existing re-exports are unchanged.
- **`crates/filecommand-tui/src/input/mod.rs`:** `map_key` dispatches on the precedence function and the slot.
- **`crates/filecommand-tui/src/input/mouse.rs`:** `context` dispatches the same way.
- **`crates/filecommand-tui/src/app.rs`:** the key dispatch that calls `map_viewer_key` and `map_editor_key` directly consults the same precedence function. Its `state.X` reads switch to accessors. The startup-warning assignments in `run` and `apply_user_menu` (last writer wins) go through a replacing setter rather than the asserting `open_overlay` (design D5).
- **`crates/filecommand-tui/src/views/mod.rs`:** `render`, `render_phase`, and `build_hitmap` are merged into one dispatch that draws and records hits.
- **`crates/filecommand-tui/src/views/`:** a `layout` function plus `DialogLayout` in each of:
  - `delete_confirm.rs`, `destination_input.rs`, `quit_dialog.rs`, `progress_dialog.rs`, `conflict_dialog.rs`, `error_dialog.rs`, `skipped_summary.rs`;
  - `file_action_menu.rs`, `menubar.rs` (titles and items).
- **Tests:**
  - about 190 references in `update/tests.rs` and about 100 in the TUI tests move to accessors or `Overlay` constructors;
  - two key-router tests that build impossible two-overlay states are rewritten (design D6);
  - new reducer tests for the slot invariants;
  - a new geometry property test for every dialog layout.
  - Insta snapshots are unchanged.

**Audit findings:** B-02, B-08

**Coordination:**
- **Land this change after the Critical and High audit fixes that touch `update_impl` and its precedence.** Those are `resize-preserves-session-state` (R-03), `listing-request-guard` (R-04, P-10), `process-lifecycle-hardening` (R-10, R-11, R-12, R-18), and `offload-blocking-io` (R-09, P-09). They are small, local fixes to the current structure. Rebasing them onto a split, restructured reducer would cost more than rebasing this refactor onto them.
- `resize-preserves-session-state` adds `State::undersized` as a screen-level flag above every phase and overlay. This change carries it into the precedence function as the top layer. The startup warning and the quit confirmation keep their keys while undersized, as that change specifies.
- `offload-blocking-io` makes viewer and editor opens reply asynchronously, which makes "a reply arrives while an overlay is open" reachable. That change already decides the rule: an open reply that arrives after a menu, dialog, or overlay has opened is discarded. With the slot its overlay check becomes `state.overlay.is_some()`. This change does not alter that rule. It only fixes the drive-list case, which is not asynchronous today (design D5).
- `config-parsing-robustness` (R-16) adds startup-warning assignments (the parser-warning summary at startup and after a user-menu edit), and `error-diagnostics` (B-06) routes every startup-warning assignment through a logging `set_startup_warning` helper. The `StartupWarning` variant stays a single `String`, and that helper becomes the replacing setter of design D5.
- `ci-and-build-tooling` (B-03) reformats the code and fixes rustdoc links to private items in `update.rs` (`update_impl`, `drag_allowed`). If it lands first, this split starts from formatted code. If not, the formatting commit is regenerated after this change.
- `streaming-listing-performance` (P-03) changes `views/panel.rs` `hit_test` and `visible_indices`. Panel row hit-testing is not part of this change (design: Non-Goals, which also records why B-08's per-row name clone in `hit_test` is kept), so the two only meet in `build_hitmap`'s dispatch.
- `filter-selection-safety` (R-17) adds a disclosure row to the delete confirmation. If it lands first, `delete_confirm`'s `layout` includes that row.
