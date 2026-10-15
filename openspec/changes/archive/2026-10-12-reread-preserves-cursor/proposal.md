# Change: reread-preserves-cursor

## Why

After the user confirms a delete (F8 or the Delete key), the panel re-reads its directory and the cursor and scroll position snap back to the top of the list — the user loses their place. Norton Commander keeps the cursor where it was, landing it on the entry that followed the deleted one. The same jump happens on every re-read of a directory the panel is already showing: copy/move/mkdir completion, Ctrl+R / menu Re-read, and a stale background tab's activation refresh. All of them funnel through `Panel::begin_new_listing`, which resets the cursor, the scroll offset, and the "user has moved" flag unconditionally, and the streamed listing then stays pinned to the first entry.

## What Changes

- A re-read of the directory a panel is **already displaying** preserves the user's position: the cursor returns to the same entry if it is still listed; otherwise to the first surviving entry at or after the cursor's previous row; otherwise to the nearest surviving entry before it. The scroll offset is restored and re-clamped so the window does not extend past the end of the shorter list and the cursor stays visible.
- This applies to every same-directory re-read: file-operation job completion (delete, copy, move, mkdir — including a cancellation after partial progress), Ctrl+R / the Left/Right menu's Re-read, and the stale-background-tab refresh on activation. When both panels browse the affected directory, both keep their own positions.
- The restored position holds **while the re-read streams in**, not only on completion, so the cursor never visibly flicks to the first row.
- Navigation into a **different** directory is unchanged: cursor and offset reset to the top and the streamed listing stays pinned until the user moves.
- Find-file's deferred cursor settle keeps precedence if both mechanisms are ever set on the same listing.
- Non-breaking. Tree mode, and preserving the quick filter or the selection set across a re-read, are out of scope (they keep today's behavior).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `panel-navigation`: adds the requirement "Same-directory re-read preserves cursor and scroll position" (cursor-anchor resolution order, scroll-offset restore and re-clamp, no transient jump while streaming, different-directory navigation unchanged); "Scroll offset is core panel state" gains the re-read anchor restore in its list of re-clamp triggers.
- `file-operations`: "Automatic panel re-read on completion" additionally states that each such re-read of a panel's current directory preserves that panel's cursor entry (or nearest survivor) and scroll offset, with a scenario for the post-delete case.

## Impact

- **Specs:** `panel-navigation` (one added requirement, one modified requirement), `file-operations` (one modified requirement, one added scenario).
- **Code:** `crates/filecommand-core/src/panel.rs` — a re-read anchor (previous entry names in display order, cursor index, scroll offset) stored on the panel, captured before a same-directory re-read and cleared by any navigation to a different directory, plus capture/apply helpers; `crates/filecommand-core/src/update.rs` — `begin_listing_inner` captures the anchor when the target path equals the panel's current directory, and `apply_listing_event` applies it on each `ListingChunk` and on `ListingComplete` (consuming it there). Tests in `crates/filecommand-core/src/update/tests.rs` and `panel.rs`. No `filecommand-tui` changes — the renderer already only reads `cursor` and `scroll_offset`.
- **Compatibility:** additive. The only observable change is that same-directory re-reads no longer reset the cursor and scroll offset.
