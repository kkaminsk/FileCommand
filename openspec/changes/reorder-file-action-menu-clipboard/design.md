## Context

The file-action menu's entry list is a plain `Vec<FileActionMenuEntry>` built once in `FileActionMenuState::open` (`crates/filecommand-core/src/dialogs.rs`). Nothing outside that constructor depends on entry order: hotkey lookup (`hotkey_action`) matches by first letter, not position, and rendering just iterates whatever order `entries` holds. This is a pure reordering with no architectural, data-model, or cross-cutting concerns.

One wrinkle: the `file-action-menu` capability's spec has not yet been archived from `openspec/changes/clipboard-file-export/` into `openspec/specs/`. This change's delta is written against that pending delta's requirement text, so it will stack correctly whenever `clipboard-file-export` is archived first (or the two are archived together).

## Goals / Non-Goals

**Goals:**
- Move `Send to clipboard` to immediately follow `Edit` in the menu's entry order for every target (file and directory-omitted variants alike).
- Leave every other aspect of the menu (which entries appear, hotkeys, routed behavior, dialog styling) unchanged.

**Non-Goals:**
- Changing what any entry does when activated.
- Changing the directory-target menu (which already omits View/Edit/Run and therefore doesn't include a "between Edit and Copy" slot the same way).

## Decisions

- **One edit point covers both the file- and directory-target menus.** `FileActionMenuState::open` already builds `entries` as one shared sequence: an `if !is_dir` block pushes `[Run?, View, Edit]`, then a common `entries.extend([Copy, Rename, Move, Delete, SendToClipboard])` tail runs regardless of `is_dir`. Moving `SendToClipboard` out of that tail to right after the `if !is_dir` block (i.e. before `Copy`) yields `[Run?, View, Edit, SendToClipboard, Copy, Rename, Move, Delete]` for a file and `[SendToClipboard, Copy, Rename, Move, Delete]` for a directory — both desired orders from a single change, with no `is_dir` branching duplicated.
- **Implementation shape**: reorder the literal push/vec sequence rather than sorting at render time — the entry list is small, fixed, and already hand-ordered; a sort comparator would be more indirection for no benefit.

## Risks / Trade-offs

- [Risk] Existing unit tests in `dialogs.rs` assert the full entry order by index/sequence and will fail once the order changes. → Mitigation: update those expected-order assertions as part of this change's implementation (tracked in tasks.md); no behavior beyond order is affected, so the fix is mechanical.
