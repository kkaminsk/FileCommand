# Tasks: mouse-panel-drag

## 1. Core state machine

- [x] 1.1 `DragState`, `DropTarget`, and commands `DragBegin`/`DragOver`/`DragDrop`/`DragCancel` in `crates/filecommand-core/src/update.rs`; items frozen at begin (mouse-drag: "Drag lifecycle"; "Robust against listing changes")
- [x] 1.2 Target validation: other panel directory, subdirectory/`..` rows in either panel, tree nodes, tabs; self/descendant/same-directory rejection (mouse-drag: "Valid drop targets")
- [x] 1.3 Refactor `enter_file_op_setup_for_sources` to take `(source_side, prefill)`; `DragDrop` opens the drop-initiated dialog (operation-dialogs: "Drop-initiated destination dialog")
- [x] 1.4 Reducer post-condition clearing `drag` on phase exit; proptest over command interleavings; no `Effect::RunJob` without the dialog path (mouse-drag: "Cancel and phase-change clear the drag")
- [x] 1.5 `panel.frame.drop` role in `theme.rs` and every built-in theme (mouse-drag: "Drag feedback")

## 2. TUI

- [x] 2.1 `MouseTracker` drag threshold (≥ 1 cell), verb from modifiers/button on each `Drag`/`Up`, `DragOver` de-duplication (mouse-drag: "Drag lifecycle"; "Verb selection"). Also closes the hit-testing gap this depends on: `HitMap`/`PanelHits` gained `tree_nodes` (path-keyed) and `tabs` (index-keyed) regions, `views::panel::hit_test` now populates them for Tree mode and any 2+-tab strip, and `views::tab_strip` gained its own `hit_test` plus a `StripCell::index` field. `cargo build --workspace` and `cargo test -p filecommand-tui` both green (324 lib tests, incl. 16 new). Drag *visuals* (frame/row/mini-status/key-bar treatment) and the drop-dialog button row are explicitly out of scope here — next stage's job (2.2/2.3).
- [x] 2.2 Drag visuals in `views/panel.rs` and `views/tab_strip.rs`; key-bar relabel in `views/keybar.rs` (mouse-drag: "Drag feedback")
- [x] 2.3 Drop-initiated dialog in `views/destination_input.rs`: add a button-row renderer (`button.normal`/`button.focused` — the view has none today) with clickable `[ Copy ] [ Move ] [ Cancel ]`, leaving the F5/F6 rendering byte-identical (operation-dialogs: "Drop-initiated destination dialog")
- [x] 2.4 Esc mid-drag cancels (mouse-drag: "Cancel and phase-change clear the drag")

## 3. Verification

- [x] 3.1 Core tests: begin → over → drop yields `FileOpSetup`; every rejection case; Move never starts without the dialog
- [x] 3.2 Snapshots: drag in progress (frame, target row, mini-status, key bar); drop dialog with Copy focused and with Move focused
- [ ] 3.3 Manual matrix on Windows Terminal, conhost, WezTerm, and GNOME Terminal including Shift and right-button drags
- [x] 3.4 `cargo build --workspace` and `cargo test --workspace` green
