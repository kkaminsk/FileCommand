# Design: reread-preserves-cursor

## Context

Every directory read in `filecommand-core` goes through `update::begin_listing` → `begin_listing_inner` → `Panel::begin_new_listing(cwd)` (`crates/filecommand-core/src/panel.rs`). That method resets `cursor = 0`, `scroll_offset = 0`, and `cursor_user_moved = false`, then the worker streams `Command::ListingChunk` events whose `insert_streamed` keeps the cursor pinned to 0 until the user moves (panel-navigation "Streamed listing keeps the top pinned until the user moves"). `Command::ListingComplete` clamps the cursor, reconciles the selection, consumes find-file's `pending_cursor_target` (if any) to settle the cursor on a named entry, and calls `reconcile_panel_viewport` to re-clamp the offset.

That reset is right when the panel navigates into a different directory. It is wrong for the three callers that re-read the directory the panel is already showing, all of which reach `begin_listing_inner` today:

- `Command::JobDone` — re-reads every active tab whose `cwd` matches the job's source or destination directory (file-operations "Automatic panel re-read on completion").
- `Command::RereadPanel` — Ctrl+R and the Left/Right menu's Re-read item.
- Stale background-tab refresh on `SwitchTab` / `CloseTab` activation (panel-tabs "Stale background tab refresh on activation").

The user-visible symptom that prompted this change: confirm a delete partway down a long listing, and the panel jumps back to the top.

## Goals / Non-Goals

**Goals:**

- No visual jump on any same-directory re-read: the cursor stays on its entry and the window stays where it was.
- Norton Commander landing rule after a delete: the entry that followed the deleted one; the one before it if the last entry was deleted; for a multi-selection delete, the first survivor at or after the cursor's old row.
- One mechanism shared by every re-read caller — no per-caller special cases.
- Core-only change, fully testable through `core::update` without a terminal.

**Non-Goals:**

- Tree display mode (its cursor and offset live in `TreeState` and it does not re-read through this path).
- Preserving the quick filter or the selection set across a re-read — `begin_new_listing` keeps clearing both; `ListingComplete`'s `reconcile_selection` is unchanged.
- Any change to navigation into a different directory, to find-file's deferred settle, or to the streaming-pinned-to-top rule for fresh directories.
- Brief-mode column geometry beyond reusing the existing `ensure_cursor_visible_brief` clamp.

## Decisions

### D1: Anchor by entry name with a survivor fallback order, not by numeric index

Before a same-directory re-read, the panel captures a `RereadAnchor { names: Vec<OsString>, cursor: usize, scroll_offset: usize }` — the previous entries' names in display order plus the cursor index and offset. When the new listing arrives, the cursor resolves to the first of these names that is present in the new listing, tried in this order: `names[cursor]`, then `names[cursor + 1..]` in order, then `names[..cursor]` in reverse. That yields "same entry if it survived, else the next survivor, else the nearest previous survivor" and covers a multi-selection delete (`{b, c, d}` deleted with the cursor on `c` lands on `e`) with no special casing.

Alternative rejected — keep the numeric cursor index and clamp: after a multi-selection delete it lands on an arbitrary entry, and it is wrong whenever the re-read changes the ordering (a copy job adding entries above the cursor).

### D2: Apply the anchor on every `ListingChunk`, consume it on `ListingComplete`

A re-read streams; if the anchor were applied only on completion, the cursor would sit on row 0 for the frames between the first chunk and the final event. Instead `apply_listing_event` runs the anchor lookup after each chunk (cheap: a name scan over the entries in hand) and once more on completion, then clears it. When the anchor applies it sets `cursor_user_moved = true`, so `insert_streamed`'s pin-to-top stops competing with it. Until a chunk containing a resolvable name arrives, the cursor stays at 0 — unobservable in practice, since the anchored entry is usually in the first chunk.

Find-file's `pending_cursor_target` is checked first on `ListingComplete` and wins if set; the two never coincide today because find-file navigates to a different directory, which clears the anchor (D4).

### D3: Restore the scroll offset, then clamp to the new list

On apply, `scroll_offset` is restored from the anchor and then clamped to `visible_len.saturating_sub(rows)` so a list that shrank at the bottom does not leave blank trailing rows (deleting the last visible entry at the end of a long listing pulls the window up by one, as Norton Commander does). The existing `reconcile_panel_viewport` then runs as it does for every other list mutation, so if the panel's body height differs from when the anchor was captured, the cursor-visibility clamp wins — the same rule panel-navigation already applies to tab restore. In Brief mode the restored offset is passed through `ensure_cursor_visible_brief`, which keeps it on a whole-column multiple.

### D4: The anchor lives on `Panel` and is cleared by any different-directory navigation

`Panel::reread_anchor: Option<RereadAnchor>` sits beside `pending_cursor_target` and follows the same hygiene: `begin_new_listing(cwd)` clears it when `cwd != self.cwd`, so a navigation never inherits a stale anchor from an earlier re-read. It is included in the per-tab stash/restore data alongside `pending_cursor_target`, so a tab switched away mid-re-read keeps its anchor for when its listing completes.

### D5: Capture in `begin_listing_inner`, so every caller benefits without plumbing

`begin_listing_inner` is the single funnel for all three re-read callers and for ordinary navigation. It captures the anchor (via `Panel::capture_reread_anchor`) only when the target path equals the panel's current `cwd` **and** the current listing is `Complete`; a re-read issued while the previous listing is still streaming captures nothing and keeps today's behavior rather than anchoring to a half-loaded list. `JobDone`, `RereadPanel`, and the stale-tab path need no changes of their own.

## Risks / Trade-offs

- An O(n) snapshot of entry names per re-read — negligible against the directory read it accompanies, and freed on completion.
- A directory that changes on disk between capture and completion resolves to whatever survivor the fallback order finds; that is the intended "nearest surviving entry" behavior, not a failure mode.
- A re-read whose previous listing was still streaming keeps the old jump-to-top; accepted as the safer choice.

## Open Questions

- Should the quick filter survive a same-directory re-read? Today it is cleared; a follow-up proposal could keep it and re-apply it to the fresh listing.
- Should the selection set survive a copy-job re-read of the source panel? Today `begin_new_listing` clears it; separate concern.
