# Tasks: path-box

## 1. Core: remove the command line

- [ ] 1.1 Delete `State.command_line`, `history`, `history_cursor` and `State::prompt()`; delete `Command::{CommandLineClear, CommandLineHistoryPrev, CommandLineHistoryNext, PasteCursorName, PasteCursorPath, HistoryDialogOpen/…}`, the panels-level `TextEdit` / `RequestPaste` arms, and the `commands` field of `Effect::PersistHistory` (command-line REMOVED; design D7, D8).
- [ ] 1.2 Delete `recall_history`, `paste_cursor_entry`, `run_command_line`, `dispatch_cd`, `complete_cd`, `dispatch_delete_builtin`, `complete_delete_builtin`, `parse_cd` / `parse_del` / `parse_rmdir` / `parse_builtin_arg` and `ProbeIntent::{Cd, Delete}` in `update/panels.rs` and `update/command.rs`; keep `resolve_cd_target`, `parse_drive_relative`, `panels_own_input`, `start_probe` / `apply_path_probed` (reshaped in 2.5).
- [ ] 1.3 Delete `HistoryDialogState`, `handle_history_dialog`, `Overlay::HistoryDialog`, `MenuAction::History`, the Commands → History item and `config::push_history` (pulldown-menus "Menu contents"; application-shell "Modal overlays are mutually exclusive by construction").
- [ ] 1.4 Remove `Keys.{paste_name, paste_path, clear_line}` and their parsing and serialisation; add `Keys.focus_path` (default `ctrl+g`) with parser tests for the default and the override (path-box "Focusing a path box": "Rebinding the focus key"; design D9).
- [ ] 1.5 The "Command line" help topic becomes "Path box" with new text (help-and-about "Help topic list": "Path box topic").
- [ ] 1.6 Update or delete the affected reducer tests in `update/tests/{panels,file_op,listing,overlays,routing}.rs`, `config.rs`, `menu.rs`.

## 2. Core: path box state and reducer

- [ ] 2.1 `path_box.rs`: `PathBoxState { side, edit: LineEdit, completion: Option<Completion>, probe: Option<u64> }`, `Completion { dir, prefix, names: Option<Vec<(OsString, bool)>>, candidates: Vec<usize>, index: Option<usize>, request: Option<u64> }`; pure helpers `split_last_segment`, `candidates_for(names, prefix)` (directories first, then case-insensitive name — its own comparator, not `cmp_entries`), `apply_candidate` with unit tests (design D3).
- [ ] 2.2 Commands `PathBoxFocus(side)`, `PathBoxEdit(TextEditOp)`, `PathBoxComplete { reverse }`, `PathBoxAccept`, `PathBoxCancel`, `PathBoxPaste(String)`, `CompletionsListed { request, dir, names }`; `Effect::ListCompletions { request, dir }`; `ProbeIntent::Select { parent, name }`.
- [ ] 2.3 Focus: set `path_box`, end type-ahead, make `side` active, text = cwd + separator, caret at end (path-box "Focusing a path box"; design D2).
- [ ] 2.4 Completion per design D3: listing-sourced and worker-sourced candidates, directories first, cycle and wrap, directory separator, prefix frozen for the cycle, cache kept on edits within the folder, stale reply and ignored-while-reading rules, `names: None` → `no matches` (path-box "Tab completes the last path segment", "Completion candidates and cycling order", "Completion cycle and mini-status", "Completion candidates come from a panel listing when possible", "Completion reads run off the event loop").
- [ ] 2.5 Accept per design D4: cwd no-op, root entry, listed parent immediate (this or the other panel), probe with `Checking …`, reply guard (same side, same text, current id), success navigates/lands/closes, failure names the path and keeps focus; Cancel resets and closes and drops the outstanding probe (path-box "Enter resolves the path box text", "Enter selects the named entry", "Enter on the panel's own directory or a root", "A listed parent answers the existence check", "The existence check never blocks the event loop", "Esc cancels the path box").
- [ ] 2.6 `ListingComplete`: exact-then-case-insensitive match for `pending_cursor_target` (design D5); find-file tests still pass.
- [ ] 2.7 Close the box whenever a layer above the panels opens (design D1): file-operation dialogs, overlays, the startup warning.
- [ ] 2.8 Reducer tests for every `path-box` scenario, run with no filesystem (application-shell "The reducer performs no filesystem I/O": all four path-box scenarios).
- [ ] 2.9 If `auto-refresh` is already merged: add `path_box.is_none()` to its probe eligibility with a test (design D10).

## 3. TUI

- [ ] 3.1 `input/mod.rs`: `map_path_box_key` (text-edit set, Ctrl+V / Shift+Ins, Tab, BackTab, Enter, Esc; everything else ignored) routed before `map_panel_key` whenever `state.path_box.is_some()`; `map_panel_key` drops `typing` and every command-line arm (design D7); `keys.focus_path` → `PathBoxFocus(active)`; `map_paste` targets the focused box or is ignored (text-field-editing deltas).
- [ ] 3.2 Mouse: click on a box → `PathBoxFocus(side)`; click elsewhere while focused → `PathBoxCancel` then normal handling; hit map `path_boxes: [Rect; 2]` (path-box "A focused path box owns the keyboard": "A click outside the box cancels it").
- [ ] 3.3 `views/path_box.rs` replaces `views/command_line.rs` (design D6): two boxes on the `cmdline` row split at the panel divider, unfocused in `Role::CommandLine` with left truncation (promote `left_truncate_with_ellipsis` from `command_line.rs` / `header.rs` to one shared helper), focused via `draw_text_field` returning the caret; `views/mod.rs` call site and cursor ownership; mini-status override for `k/n matches` / `no matches` / `Reading…` in `views/panel.rs`.
- [ ] 3.4 `worker.rs`: `spawn_completion_list` using `listing::list_children_for_completion` (every readable child, no `..`); `app.rs` runs `Effect::ListCompletions`; `spawn_path_probe` unchanged.
- [ ] 3.5 `app.rs`: stop loading `history.commands` into state; `history_writer` keeps the file's `commands` array (design D8); remove `views/history_dialog.rs` and its routing.
- [ ] 3.6 Tests: `input/tests.rs` (box keys, panel keys no longer typing-dependent, Ctrl+G, rebinding, unbound keys ignored in the box), `input/mouse/tests.rs`, the `app.rs` burst test retargeted to a focused box, `snapshot_views.rs` (unfocused boxes both sides, focused box with caret, long-path truncation, matches mini-status, `Reading…`), regenerate the full-frame snapshots, `tests/integration_smoke.rs`.

## 4. Docs

- [ ] 4.1 README: replace the "Command line" section and its "History" subsection with "Path box"; key tables (Ctrl+G; Tab / Shift+Tab / Enter / Esc in the box; drop Ctrl+Enter, Ctrl+], Ctrl+Y, Alt+F8); `key.*` list; note the removed builtins and that Ctrl+O is unchanged.
- [ ] 4.2 F1 Help: "Path box" topic page; keyboard reference drops Alt+F8 and the paste keys; configuration topic lists `key.focus_path` and drops `key.clear_line`.

## 5. Verify

- [ ] 5.1 `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean; `openspec validate path-box --strict` clean.
- [ ] 5.2 Manual: Ctrl+G, type `..\`, Tab cycles siblings; type a prefix, Tab, Enter → cursor lands; `D:\` Enter → enters the root; a UNC share shows `Reading…` / `Checking …` without freezing; Esc reverts; Tab over the panels still switches; Backspace still goes up; `+` still opens group select; an old config with `key.clear_line` warns at startup.
