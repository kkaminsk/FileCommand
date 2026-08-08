# Design: enter-file-action-menu

## Context

Enter on a panel entry currently has three specified outcomes: descend into a directory (`panel-navigation`), run the typed command when the command-line buffer is non-empty (`command-line`), and spawn an executable target via the suspended-TUI shell (`command-line`). Enter on a non-executable file is a no-op (`filecommand-core::update::handle_enter` returns no commands for non-executable files). All per-file actions today require knowing the F-key bindings (F3–F8), and every mutating operation already flows through the M2 dialog set: destination input, overwrite conflict, progress, error recovery, delete confirmation, skipped-files summary.

This change gives Enter-on-file a purpose: a small modal menu of the common actions for the entry under the cursor, reusing the existing flows end to end.

## Goals / Non-Goals

**Goals:**

- Enter on any file entry (empty command buffer) opens a modal action menu for that entry.
- Menu actions reuse the existing viewer, editor, copy/move destination, and delete-confirmation flows unchanged — the existing dialogs are the confirmation layer; no new "Are you sure?" step is introduced.
- Executables remain runnable via the menu (Run entry) using the existing suspended-spawn path.
- Provide an in-place Rename distinct from Move.

**Non-Goals:**

- No change to Enter on directories or `..`, to non-empty-command-buffer Enter, or to Ctrl+Enter/Ctrl+] paste bindings.
- No multi-selection semantics — the menu acts on the cursor entry only.
- No "open with Windows file association" (ShellExecute) action; out of scope for this change.
- No changes to the F2 user menu, the F9 pull-down menus, or the F3–F8 direct bindings, which all keep working as before.

## Decisions

### D1: The menu opens for all files, including executables

Enter on an executable opens the same menu with a **Run** entry listed first, rather than spawning directly. Rationale: one consistent Enter behavior for every file; running stays one keypress away (Enter, Enter) because Run is the first, default-highlighted entry. Alternative considered: keep direct spawn for executables and show the menu only for non-executables — rejected as two behaviors for one key, and it leaves no keyboard route to copy/rename/delete an executable via Enter.

### D2: Rename and Move are separate menu entries

Rename opens an input dialog pre-filled with the entry's current name and renames within the current directory; Move opens the existing F6 destination dialog pre-filled with the opposite panel's path. Rationale: the two intents are distinct, and NC's combined F6 makes the common "just fix the name" case awkward. Rename reuses the existing same-volume rename machinery, including identity-aware case-only rename. Alternative considered: a single combined Rename/Move entry mirroring F6 — rejected per user decision.

### D3: Cursor entry only; selection is not consumed

The menu always targets the entry under the cursor, ignoring any multi-entry selection, and leaves the selection intact. Rationale: Enter is a cursor action (like Enter-on-directory); F5/F6/F8 remain the selection-aware operations. Alternative considered: menu acts on the selection when one exists — rejected as surprising (Enter on file A deleting files B–Z) and redundant with the F-keys.

### D4: Existing dialogs are the confirmation layer

Copy and Move route into the F5/F6 destination-input dialog (destination pre-filled with the opposite panel path), Delete routes into the F8 delete-confirmation dialog (permanent-delete warning), and Rename's input dialog is its confirmation. Overwrite conflicts, per-file errors, progress, and the skipped-files summary reuse `operation-dialogs` untouched. Rationale: every mutating action already requires an intervening dialog; a second explicit confirm would double-prompt. Esc at the menu or at any dialog leaves the filesystem untouched. Alternative considered: an extra Yes/No confirm after choosing a menu action — rejected per user decision.

### D5: Menu presentation follows existing modal-menu conventions

Primary-style dialog (§4.4: black on cyan, double-line frame), Up/Down moves the highlight, Enter activates, Esc closes without action, first-letter hotkeys activate directly — matching the `user-menu` and `pulldown-menus` interaction model. Menu state lives in `filecommand-core` and all mutations flow through `core::update`, same as every other dialog; the TUI contributes a snapshot-tested view and modal input routing.

### D6: The command-line spec keeps ownership of the spawn path

The suspended-TUI spawn requirement stays in `command-line`; its executable-target sentence is modified to route through the menu's Run entry instead of spawning on Enter directly. Rationale: the spawn/terminal-restore semantics are already specified there and unchanged; only the trigger moves. The design doc's key table (`docs/superpowers/specs/2026-08-06-filecommand-design.md` §5, "Enter … run executable") becomes historical on this point; OpenSpec specs are the source of truth and the design doc is not edited by this change.

## Risks / Trade-offs

- [Muscle-memory break: NC users expect Enter to launch executables immediately] → Run is the first and default-highlighted entry, so Enter-Enter launches; the change is called out as **BREAKING** (behavior) in the proposal.
- [Modal-input regressions: another modal layer over panels] → reuse the existing dialog modality plumbing and reducer patterns; snapshot tests for the menu view and reducer tests for open/navigate/activate/dismiss.
- [Rename collides with an existing name] → the rename job surfaces the existing overwrite-conflict/error dialogs from `operation-dialogs`; no new conflict UI.
- [Divergence from the design doc's key table] → noted in D6; the delta specs govern.

## Open Questions

- None. Menu contents, executable handling, Rename/Move split, and the confirmation model were settled with the user before authoring.
