# Change: path-box

## Why

The row above the F-key bar is a Norton Commander command line, but FileCommand never runs typed text through a shell: the buffer only recognises `cd`, `del` and `rmdir`, and everything else is an error. What remains is a typing sink that silently changes what Backspace, Up/Down, Left/Right, Enter and `+`/`-`/`*` do depending on whether it holds text, plus history, paste-name and clear-line machinery that exists only to serve those three verbs. Meanwhile the one thing the row is good for — getting to a path by typing it — has no completion, so a long path must be typed in full, and the panel can only *enter* a folder, never land on a file.

This change replaces the command line with two **path boxes**, one under each panel, that show the panel's folder and complete folder and file names on Tab. Pressing Enter on a valid path puts the panel cursor on that entry. Everything else the command line did is removed.

## What Changes

- **The command-line row becomes two path boxes.** The row above the F-key bar splits at the panel divider. Each box shows its panel's current folder followed by a path separator (`C:\Github\FileCommand\`) and follows the panel through navigation, tab switches and panel swaps. Unfocused boxes are drawn in the existing `commandline` theme role; a long folder left-truncates with a leading `…`.
- **A box must be focused to type into it.** Ctrl+G (rebindable via `key.focus_path`) focuses the active panel's box; a mouse click on either box focuses that box and makes its panel active. While a box has focus it owns the keyboard and the terminal cursor and is drawn in the dialog text-field style. Esc, or a click outside the box, returns focus to the panel and resets the box to the panel's folder. While no box is focused the panels keep every key meaning they have today — Tab still switches panels, Backspace still goes to the parent, `+`/`-`/`*` always select — and plain printable keys over the panels are ignored.
- **Tab completes a path segment.** The text before the last separator names a folder; the text after it is a case-insensitive prefix. Tab replaces that last segment with the next child of the folder whose name starts with the prefix (folders first, then files, each sorted by name); Shift+Tab goes back; both wrap. A completed folder gets a trailing separator so the next Tab descends into it. The panel's mini-status shows `3/12 matches`, `no matches`, or `Reading…` while the folder is being read. Candidates come from a panel's own listing when the folder is that panel's current folder, otherwise from a directory read on a worker thread; a reply that no longer matches the box text is discarded.
- **Enter selects the entry.** The text resolves to an absolute path with the same rules the Copy/Move destination field uses (relative, `..`, drive-relative `D:x`, UNC). Its existence is answered from a panel listing when possible, otherwise by the existing off-thread path probe with a `Checking …` note. On success the panel navigates to the entry's parent folder if it is not already there and lands the cursor on the entry; a filesystem root, which has no parent, is entered instead; text naming the panel's own folder just closes the box. Focus returns to the panel. On failure the panel shows a not-found error and the box keeps focus and text.
- **BREAKING — removed:** the `cd`, `del` and `rmdir` builtins; command history (Up/Down recall, the Alt+F8 history dialog, Commands → History); Ctrl+Enter / Ctrl+] paste-name/paste-path; Ctrl+Y clear-line; the `key.paste_name`, `key.paste_path` and `key.clear_line` config keys (a config that still sets them gets the standard unrecognised-key startup warning). `history.json` keeps its format: the `commands` array is preserved on write but never appended to; directory history for Ctrl+J is unchanged.
- **Kept, re-homed:** Ctrl+O panels-on/off and the suspended-TUI shell run used by the file-action menu's Run entry and the F2 user menu (including the configurable shell) move from the `command-line` capability to `application-shell`, unchanged in behaviour.
- **Docs:** the F1 Help topic `Command line` becomes `Path box`; README key tables and the Command line section are rewritten.
- **Non-goals:** a candidate popup list; completion history; routing plain printable keys into the box or into type-ahead; any change to Ctrl+J fuzzy jump, Alt+letter type-ahead or the Ctrl+P quick filter.

## Capabilities

### New Capabilities

- `path-box`: the two path boxes under the panels — their rendering, focus and editing, Tab completion, Enter selection, Esc, and the `key.focus_path` binding.

### Modified Capabilities

- `command-line`: **retired.** Every requirement is REMOVED; Ctrl+O and the suspended shell run are re-added under `application-shell`. The capability directory is deleted when the change is archived.
- `application-shell`: ADDED "Panels on/off reveals terminal scrollback" and "Configurable shell with documented latency tradeoff" (moved verbatim from `command-line`), plus "Shell commands run only from the Run entry and the user menu" and "A suspended shell run restores the terminal" (the former command-line "Run command via shell in suspended-TUI mode", split in two and reworded for its new home); MODIFIED "Terminal ownership and restoration on every exit", "Resize handling with 60x16 hard floor and placeholder", "Quit request keys and confirmation" (Esc in a focused path box cancels the box), "Bursts of key input render once", "The reducer performs no filesystem I/O", "Slow storage never blocks the event loop", "Modal overlays are mutually exclusive by construction" (the history dialog is gone).
- `panel-navigation`: MODIFIED "Parent-directory navigation" (Backspace is unconditional), "Swap panels (Ctrl+U)", "Superseded directory listings are discarded and cancelled" (scenario wording).
- `file-action-menu`: MODIFIED "Enter on a file opens the action menu" (no command-buffer precedence), "Menu actions route to existing flows" (wording).
- `text-field-editing`: MODIFIED "Caret model and editing keys", "Paste into a text field", "Terminal paste arrives as one paste" (the path box is the panels-level text field; no exceptions list).
- `type-ahead-jump`: MODIFIED "Entering type-ahead mode with Alt+letter", "Extending the pattern with printable keys"; RENAMED "Exiting type-ahead and restoring command-line routing" → "Exiting type-ahead", with its text MODIFIED (printables are ignored after exit).
- `responsive-layout`: MODIFIED "Degraded band and per-panel breakpoints" (wording), "Chrome degradation" (path-box scrolling and truncation).
- `quick-filter`: MODIFIED "Cursor and mini-status behavior under an active filter" (drops the paste bullet).
- `help-and-about`: MODIFIED "Help topic list" (`Path box`).
- `pulldown-menus`: MODIFIED "Menu contents" (Commands loses History).
- `startup-splash`: MODIFIED "Minimum hold and key dismissal" (wording).
- `user-menu`: MODIFIED "Navigate and dismiss the user menu" (wording).
- `theme-selection`: MODIFIED "Live theme preview while the picker is open" (wording).
- `filesystem-error-handling`: MODIFIED "Error messages name the affected path" (wording).

## Impact

- **Core (`crates/filecommand-core`):** remove `State.command_line` / `history` / `history_cursor`, the `CommandLine*`, `PasteCursor*`, `HistoryDialog*` commands and the panels-level `TextEdit` / `RequestPaste` arms, `ProbeIntent::{Cd, Delete}`, `run_command_line` and the builtin parsers in `update/panels.rs`, `HistoryDialogState`, the History menu item. Keep `resolve_cd_target` / `parse_drive_relative` (destination dialog), `panels_own_input` (viewer), `Role::CommandLine`, the `HistoryFile` reader/writer. Add `State.path_box: Option<PathBoxState>` (new `path_box.rs`), `Command::{PathBoxFocus, PathBoxEdit, PathBoxComplete, PathBoxAccept, PathBoxCancel, PathBoxPaste, CompletionsListed}`, `Effect::ListCompletions`, `ProbeIntent::Select`, `Keys.focus_path`; `pending_cursor_target` matching gains a case-insensitive fallback.
- **TUI (`crates/filecommand-tui`):** `views/command_line.rs` → `views/path_box.rs` (two boxes, reusing `draw_text_field` and `left_truncate_with_ellipsis`); `input/mod.rs` drops the `typing` gate and all command-line arms, adds a `map_path_box_key` routed ahead of the panel map; mouse: box click; `worker.rs` gains a completion-list reader; `app.rs` stops loading command history into state (the writer keeps the file's `commands`); hitmap `cmdline` → `path_boxes`; `views/history_dialog.rs` removed.
- **Tests:** every command-line test in `update/tests/{panels,file_op,listing,overlays,routing}.rs`, `input/tests.rs`, `config.rs`, `menu.rs`, the `app.rs` burst tests, and the `snapshot_views.rs` command-line and history-dialog snapshots; 66 of the 138 snapshot files regenerate because the prompt row changes; new reducer tests for the path box; new snapshots for unfocused and focused boxes and the matches mini-status.
- **Docs:** README (the "Command line" section and its "History" subsection, key tables, `key.*` list), F1 Help topic text.
- **Persisted formats:** `config.toml` loses three `key.*` keys and gains `key.focus_path`; `history.json` format unchanged.
- **Breaking** for users who typed `cd` / `del` / `rmdir`, used Alt+F8, or rebound the removed keys.
