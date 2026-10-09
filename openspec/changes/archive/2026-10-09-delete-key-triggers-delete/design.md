## Context

`crates/filecommand-tui/src/input/mod.rs::map_panel_key` translates panel keystrokes to core `Command`s. Its `match key.code` block maps `KeyCode::F(8) => Some(Command::RequestDelete)`, but it has **no `KeyCode::Delete` arm**. A Delete keypress therefore falls through the entire match to the trailing `_ => None`, and no command is issued — the key is dead in panels.

The delete pipeline downstream of `RequestDelete` is healthy and reached by every other entry point: F8, the `Files` pull-down, the file-action menu, mouse activation, and the `del`/`rmdir` command-line verbs all funnel into `RequestDelete` → the `DeleteConfirm` dialog → `FileOpConfirm`. `KeyCode::Delete` is already recognized elsewhere in this file — `key_name` (mod.rs) maps it to `"delete"` for config-binding matching — so the enum variant exists and is understood; it is simply absent from the compiled-in panel keymap.

The bug shipped without detection because `input/tests.rs` asserts `F(8) → RequestDelete` but has no equivalent assertion for `KeyCode::Delete`.

## Goals

- Pressing the Delete key in a panel behaves exactly as pressing F8: it enters the existing delete-confirmation flow for the cursored item or the current selection.
- Zero behavior change to the delete flow itself, its confirmations, or its permanent-deletion semantics.
- A regression test locks in the Delete-key binding.

## Non-Goals

- No new rebindable configuration entry for delete (the `Keys` config struct is untouched).
- No `Shift+Delete` / recycle-bin variant — deletion in FileCommand is already permanent by design (`operation-dialogs` "Delete confirmation dialog").
- No changes to the viewer, editor, or any dialog/overlay keymap; this is a panel-context binding only.

## Decisions

### D1 — Fixed alias, not a configurable binding
The Delete key is a hard-coded alias for F8, mirroring the existing fixed `Ctrl+Ins` → clipboard-Files alias in the same function (which is deliberately not rebindable). This is the smallest possible surface and matches Norton Commander, where Del maps to the F8 action. Rejected: adding a `delete` field to the `Keys` config struct — more surface (schema, defaults, docs) for no requested benefit.

### D2 — Plain Delete only
The new arm is guarded by an exact no-modifier check (`key.modifiers == KeyModifiers::NONE`, deliberately stricter than `is_plain`, which still admits Shift) so that only an unmodified Delete triggers deletion; a modified Delete press falls through, keeping `Shift+Delete` and `Ctrl+Delete` available for any future meaning. This matches how the plain F8 arm and the other unmodified panel keys behave.

### D3 — Placement in the match
The arm is added among the unmodified navigation/function keys near the `KeyCode::F(8)` arm, i.e. after the ctrl/alt-guarded arms earlier in the function, preserving the file's existing "guarded chords first, then plain keys" ordering.

## Risks / Tradeoffs

Negligible — the change is purely additive and reuses an already-tested command path. The only conceivable risk is shadowing a future modified-Delete chord, which the no-modifier guard prevents.

## Open Questions

None.
