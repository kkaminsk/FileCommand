# Design: filter-selection-safety

## Context

This was verified against `build/open-proposals-2026-10` at `1bada6f`.

**Quick filter (`crates/filecommand-core/src/panel.rs`).**

- `PanelState::quick_filter: Option<String>`.
- `matches_quick_filter` is a case-insensitive substring test, and the `..` entry always matches.
- `visible_indices()` allocates the matching indices.
- `quick_filter_push`/`quick_filter_backspace` call `snap_cursor_to_visible`. That function **returns early when `visible` is empty**, and otherwise moves the cursor to the nearest visible index.
- `move_cursor_filtered` returns early on an empty visible list.
- `jump_to_prefix` (Alt+letter), `handle_click_entry`, and `handle_open_action_menu_at` already target visible entries only: the first by searching the visible subset, the other two because mouse hit maps contain only rendered rows.

So the only way the cursor rests on a hidden entry is a pattern that matches nothing, in a listing without `..`. That means a drive root or UNC share root, or an empty-result case where `..` is absent.

**Cursor consumers.**

- `PanelState::selected()` is `self.entries.get(self.cursor)`, with no filter check. Every cursor-targeted reducer path goes through it:
  - `handle_enter` (Enter, including opening the file-action menu);
  - `handle_request_viewer` (F3);
  - `handle_request_editor`/`handle_request_external_editor` (F4);
  - `active_selection_sources` when nothing is selected (F5/F6/F8/Delete, and Ctrl+C clipboard export);
  - `paste_cursor_entry` (Ctrl+Enter/Ctrl+]).
- The TUI's mini-status and the opposite panel's quick view also call `selected()` (`views/panel.rs`).
- `toggle_selection_and_advance` (Ins) bypasses `selected()` with its own `entries.get(self.cursor)`, so Ins on an empty filtered view toggles a hidden entry.
- `set_sort_mode` also reads `entries.get(cursor)`, but only as a sort anchor, which is harmless.

**Selection under a filter.**

- `active_selection_sources` returns every entry in `panel.selected` when the set is non-empty, whether or not the filter hides it.
- `enter_file_op_setup_for_sources` builds `FileOpSetup::DestinationInput { kind, sources, source_dir, input, buttons }` for F5/F6, for file-action-menu Copy/Move, and for drag-and-drop drops.
- `enter_delete_confirm_for_sources` builds `FileOpSetup::DeleteConfirm { sources, … }`.

Rendering:

- `views/delete_confirm.rs` draws a 4-row box: a frame, `Delete <name | N files>? This cannot be undone.`, and `(Y)es (N)o`. Its `hit_buttons` re-derives the same geometry.
- `views/destination_input.rs` draws a 6-row box, or 7 rows with drop buttons: frame and title, blank, prompt, field, **blank (row 4)**, optional buttons, frame.

**Wildcards.** `select_matching` (`+`) and `deselect_matching` (`-`) both call `panel::wildcard_match(pattern, &entry.name.to_string_lossy())`, so they share one matcher. That matcher:

- lowercases both strings;
- recurses over `as_bytes()`, so `?` consumes a single byte;
- has a `*` arm that tries both "skip `*`" and "consume one byte" recursively, which is exponential in the number of stars on adversarial input.

## Goals / Non-Goals

**Goals:**

- No keyboard command ever acts on a cursor entry the user cannot see.
- Selection-scoped operations keep acting on the whole selection (user decision), and they say so whenever part of that selection is hidden.
- `+`/`-` patterns treat each character as one character, and no pattern can stall the UI.
- Unfiltered panels and dialogs without hidden selections stay byte-for-byte unchanged.

**Non-Goals:**

- Changing which entries `+`, `-`, and `*` (invert) affect while a filter is active. They operate on the whole listing, as today, consistent with quick-filter "the quick filter narrows what is shown but does not alter the underlying listing". The disclosure covers the consequence when an operation follows.
- Disclosing hidden selections for clipboard export (Ctrl+C, Send to clipboard). That path has no confirmation dialog, and its feedback count already includes every selected entry.
- Making matching run against the raw `OsString` instead of `to_string_lossy()`. selection "Select a group by wildcard with `+`" says matching runs against the original stored name, but `select_matching`/`deselect_matching` already pass `to_string_lossy()`. The two differ only for names that are not valid Unicode (an unpaired UTF-16 surrogate on Windows becomes one U+FFFD), where `?` still consumes one unit and `*` still spans it. This existing drift is left as is and is not widened by the rewrite.
- Caching visible indices (`streaming-listing-performance`, P-03).
- Any change to operation-dialogs "Destination input dialog" (in flight in `line-editing-in-text-fields`) or "Delete confirmation dialog".

## Decisions

### D1: `selected()` is filter-aware; the panel has no current entry when nothing is visible

`PanelState::selected()` returns `None` when `quick_filter` is `Some(p)` and the cursor entry does not satisfy `matches_quick_filter(entry, p)`. Otherwise it behaves as before. The check costs one string match, not a `visible_indices()` allocation.

`toggle_selection_and_advance` starts with `let Some(entry) = self.selected()` and does nothing to the selection when there is no current entry. The following `move_cursor` is already a no-op on an empty visible list.

Because every cursor-targeted command already treats `None` as "do nothing", one change closes all of them:

- Enter, F3, F4;
- F5/F6/F8/Delete when nothing is selected;
- Ctrl+C on the cursor entry, which then reports its existing `Nothing to copy` feedback instead of copying the hidden entry;
- Ctrl+Enter/Ctrl+];
- the TUI's opposite-panel quick view, which shows nothing.

`cursor` itself is left where it is. When the pattern changes so that entries become visible again, `snap_cursor_to_visible` moves it to the nearest visible entry, as today. If the old cursor entry is visible again, it simply stays there.

Alternatives considered:

- **A sentinel cursor value (`usize::MAX`) or `Option<usize>` cursor.** This touches every cursor user (rendering, scrolling, tabs, re-read anchors) for a state that only exists while a filter matches nothing.
- **Clearing the filter automatically when it matches nothing.** This contradicts quick-filter "No matches yields an empty body", which keeps the panel in filter mode.
- **Guarding each command handler individually.** This needs many call sites, and a future handler would forget the guard.

### D2: The hidden-selection count is computed when the dialog opens and stored on the setup

`FileOpSetup::DestinationInput` and `FileOpSetup::DeleteConfirm` gain `hidden_by_filter: usize`.

- `enter_file_op_setup_for_sources(state, kind, sources, source_side, …)` and `enter_delete_confirm_for_sources` compute it from the **source panel** with a new helper, `PanelState::hidden_selected_count(&[SourceItem])`. A source counts when it lies in that panel's `cwd`, its name is in the panel's selection set, and its entry fails the active `quick_filter`. With no filter the count is 0.
  - Restricting the count to selected entries keeps the wording ("selected … hidden by filter") true for every caller. `enter_delete_confirm_for_sources` is also reached from the command-line `del`/`rmdir` builtins (`dispatch_delete_builtin`), whose target the user typed. Such a target counts only if it also happens to be a selected entry of the panel, and then the disclosure is accurate.
  - A single-target file-action menu (Enter-opened, or right-clicked on an unselected entry) always targets a visible entry, so its count is 0.
- Every other place that rebuilds these variants (the setup-dialog state transitions in `update.rs`: `FileOpConfirm` toggling `confirmed_once`, text edits, and the drop dialog's verb change, plus `confirm_destination_input`'s early return that keeps the dialog open) carries the field through unchanged.

The count is stored rather than recomputed at render time because the filter can no longer change while the modal dialog is open. Storing it also keeps the TUI free of filter logic and keeps `hit_buttons` geometry a pure function of the setup.

Operations still use the full `sources` list. Nothing about job execution changes (user decision).

Alternative considered: excluding hidden entries from operations, the audit's NC-style suggestion. The user rejected it. Select-then-filter-then-act is a deliberate workflow, and silently shrinking a selection would be its own surprise.

### D3: Disclosure wording and placement

The text, used only when `hidden_by_filter > 0`:

- With several sources: `{h} of {n} selected hidden by filter`, for example `2 of 5 selected hidden by filter`.
- With one source (a selection of one entry that the filter hides): `Selected item is hidden by filter`.

Placement:

- **Delete confirmation.** The text goes on its own row between the question and the `(Y)es (N)o` row, so the box grows from 4 to 5 rows. The question line is untouched, so the "names the single item / shows the count / states the deletion is permanent" contract of operation-dialogs "Delete confirmation dialog" holds verbatim. `hit_buttons` uses the same row count: the button row moves down one row when the disclosure is present. Both the preferred and the minimum height passed to `overlay_rect` become 5, still within responsive-layout "Unified overlay geometry"'s 58×14 cap on declared minimums, so the dialog stays renderable at the 60×16 floor. Both functions derive the row layout from one shared helper, so they cannot drift (pending `reducer-overlay-structure`, B-08). The row also appears on the second, non-empty-directory confirmation.
- **Destination dialog** (keyboard F5/F6, file-action-menu Copy/Move, and drop-initiated). The text goes on the existing blank row directly below the input field (row 4), truncated with an ellipsis to the inner width. The box size, the field position, the caret position, and the drop button row are all unchanged, so the in-flight text-field rendering and the mouse hit-testing are unaffected. operation-dialogs "Drop-initiated destination dialog" requires keyboard-initiated dialogs to stay unchanged by the drop feature ("F5 dialog unchanged": no button row). The disclosure adds no button row, appears only when a selected entry is hidden, and the selection requirement states it is additive, so the two do not conflict.
- Both use the dialog's body role. No new theme role is introduced.

Alternatives considered:

- **Appending the count to the delete question** (`Delete 5 files (2 hidden)? …`). In narrow terminals the question is truncated with `…`, so the disclosure would be the first thing cut.
- **Changing the destination dialog title** (`Copy (2 hidden)`). The drop dialog's title format is specified by operation-dialogs, and the keyboard title is fixed per verb.

### D4: Iterative, character-based wildcard matching

`wildcard_match(pattern, name)` collects `pattern.chars()` and `name.chars()` into `Vec<char>` (names are at most a few hundred characters) and runs the classic two-pointer glob:

- On a literal or `?` match, both indices advance. `?` matches any one `char`.
- On `*`, it records `star = p`, `mark = s`, and advances the pattern index.
- On a mismatch with a recorded star, it backtracks to `p = star + 1`, `s = mark + 1`, and sets `mark += 1`.
- After the name is consumed, the remaining pattern must be all `*`.

Only the most recent `*` is ever backtracked to, which is safe because an earlier `*` can never need to absorb more once a later one exists. The worst case is O(|pattern| × |name|), with no recursion and no exponential blow-up. Characters are compared case-insensitively per character: equal, or `a.to_lowercase().eq(b.to_lowercase())`. That avoids pre-lowercasing whole strings, which can change the character count (`İ` lowercases to two scalars) and would break "`?` is one character".

The audit calls this algorithm "linear". It is linear for typical patterns, but its worst case is the product of the two lengths. The spec states that accurate bound.

Alternative considered: segment matching with substring search (KMP between stars) for a strict O(n + m). It is more code for no observable gain on file names.

## Risks / Trade-offs

- **[Risk] A user at a drive root types a non-matching pattern, presses F8 out of habit, and sees nothing happen.** → This is intentional, and the empty panel body explains it. With a non-empty selection, F8 still opens the confirmation with the disclosure.
- **[Trade-off] The delete confirmation box is one row taller when hidden entries are selected.** → It happens only in that case. The F8 dialog is otherwise unchanged and its snapshots stay byte-identical.
- **[Risk] Per-character case folding differs from whole-string lowercasing for a few special characters** (for example `ß` against `SS`). → Whole-string lowercasing never matched `ß` against `SS` either. For a single-character fold, the behavior is identical to today.
- **[Risk] `selected()` returning `None` in a state that used to return `Some` could surprise a caller that assumed `Some` whenever `entries` is non-empty.** → A grep shows that every caller already handles `None`. The unit tests pin each user-visible command's no-op.

## Migration Plan

None. These are behavioral fixes with no persisted state.

## Open Questions

None.
