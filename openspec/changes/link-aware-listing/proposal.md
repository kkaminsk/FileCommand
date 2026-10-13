# Change: link-aware-listing

## Why

Two audit findings live in the panel's directory reader, `StdFsReader::read_dir` (`listing.rs`).

**R-05 (High): junctions and directory symlinks list as files and cannot be entered.** The reader classifies each entry with `DirEntry::metadata()`, which never follows a reparse point. Rust reports every name-surrogate reparse point (junction or symlink) with `is_dir() == false`. A probe against a `mklink /J` junction printed `is_dir=false is_symlink=true attrs=0x410`. Unix `lstat` behaves the same way for a symlink to a directory. A junction therefore becomes `EntryKind::File`:

- Enter opens the file-action menu instead of descending.
- F3 tries to view a directory.
- The Size column shows `0` instead of `▶SUB-DIR◀`.

This breaks panel-navigation "Enter descends into a directory" for pnpm `node_modules` (junction-based on Windows), profile junctions (`Application Data`, `My Documents`), `C:\Users\All Users`, and developer symlinks.

**R-21 (Low): one unreadable entry fails the whole listing.** `let entry = entry?; let metadata = entry.metadata()?;` turns a single entry whose metadata cannot be read into `ListingFailed` for the entire directory. On Unix the classic case is a file deleted between `readdir` and `lstat` in a busy temp or build directory. On Windows, `DirEntry::metadata()` is served from the enumeration's own find data and does no I/O. A probe deleted an entry mid-iteration and its metadata still read fine. So on Windows the per-entry failure comes only from the new link-following step this change adds, and that step handles failure itself (a dangling link lists as a file).

## What Changes

- **Directory links list as directories.** When an enumerated entry is a link (`file_type().is_symlink()`: junctions and symlinks on Windows, symlinks on Unix), the reader follows it with one `std::fs::metadata` call to classify it:
  - A link to a directory lists as a directory: directory style, `▶SUB-DIR◀`, Enter descends.
  - A link to a file lists as a file with the target's size.
  - A dangling or unfollowable link lists as a non-enterable file entry instead of failing the listing.
  - The name, date/time and hidden flag still come from the link entry itself.
  - Entering a directory link makes the link path the panel's directory. `..` returns to the directory that contains the link, not the target's parent.
- **Entries carry an internal link flag** (`RawDirEntry::link`, `Entry::link`). The flag is not rendered. Two consumers use it:
  - Find-file (Alt+F7) does not descend into directory links, which keeps today's behavior and avoids junction cycles.
  - The F8 delete confirmation asks once, not twice, for a directory link, because Delete removes only the link. This keeps today's behavior: junctions were files before this change.
- **One unreadable entry does not fail a listing.** An entry whose own metadata cannot be read is skipped and counted. On completion, a listing that skipped N entries shows `N entries could not be read` (or `1 entry could not be read`) on the panel's mini-status line, through the existing inline-message surface (`last_error`). The message stays until that panel next lists a directory. An error from the enumeration itself, not tied to one entry, still fails the listing with the inline read-error state, as today.
- Tree mode's lazy child read and Info's directory counts use the same reader, so they also see directory links as directories and stop failing on an unreadable entry. Nothing else changes for them.
- Non-breaking. No rendering changes, so no snapshot changes. A link marker in the entry row is out of scope, because "Entry row rendering" is being modified in flight.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `panel-navigation`: new requirements "Directory links list as directories" and "One unreadable entry does not fail a listing." "Streaming listing mini-status" is modified: its "revert on completion" rule gains the skipped-entries exception, and its existing scenarios are kept. None of the in-flight requirements ("Entry row rendering", "Full display mode layout", "Scroll offset is core panel state") is modified.

## Impact

- **Specs:** `panel-navigation` (two added requirements, one modified requirement).
- **Code:**
  - `crates/filecommand-core/src/listing.rs`:
    - `RawDirEntry` and `Entry` gain `link: bool`.
    - `FsReader::read_dir` returns `RawDirListing { entries, unreadable }`.
    - `StdFsReader::read_dir` (lines 465-482) skips and counts unreadable entries, and follows links through a small pure classification helper.
    - `list_dir_chunked` (lines 499-513) returns `ListingTotals { entries, unreadable }`.
    - `list_child_dirs` and `find_in_subtree` (lines 525-583) consume `.entries`, and `find_in_subtree` skips descent into links.
  - `crates/filecommand-core/src/update.rs`:
    - `Command::ListingComplete` gains `unreadable: usize`.
    - `apply_listing_event` (lines 3846-3912) sets the mini-status message.
    - `enter_delete_confirm_for_sources` (lines 2751-2756) excludes directory links from the second-confirmation trigger.
  - `crates/filecommand-tui/src/worker.rs`: `spawn_listing` (lines 178-203) forwards the count, and `gather_info` (lines 102-124) reads `.entries`.
  - Every `Entry { .. }` and `RawDirEntry { .. }` struct literal in core and tui tests gains `link: false`, as `hidden-entry-styling` did for `hidden`.
  - The `FsReader` test doubles (`listing.rs` `FakeReader`/`TreeReader`, `tui/worker.rs` `FakeReader`) adopt the new return type.
- **Out of scope:** `fs_ops`' job enumeration (`RealFs::read_dir`, `fs.rs:177-187` and `311-317`) has the same `entry.metadata()?` pattern. See design D6 for why it stays as is.

**Audit findings:** R-05, R-21

**Coordination:**
- `listing-request-guard` (R-04, P-10) adds a request id to `StartListing` and the three listing commands. That is a textual overlap on `ListingComplete` and `spawn_listing`, with no semantic conflict.
- `streaming-listing-performance` (P-01..P-04) adds a streaming `FsReader::for_each_entry` (with `read_dir` collecting through it) and moves `Entry` construction to an `Entry::new(..)` constructor with a private sort key. Whichever change lands second rebases. If streaming lands first:
  - `classify_entry` runs inside `StdFsReader::for_each_entry`'s per-entry loop.
  - The unreadable count becomes that method's `Ok` value and rides on `ListingComplete`.
  - `link` becomes an `Entry::new` parameter rather than a struct-literal field.

  The behavior in this proposal is unchanged either way.
- `move-reparse-point-safety` makes `fs_ops` treat reparse points as leaves when a move removes its source. It is independent: `fs_ops` classifies from its own `FsMetadata` and does not read `Entry::link`. It also fixes a pre-existing `RealFs` defect where Delete of a junction fails with "Access is denied." Junctions become far more visible and actionable once they list as directories, so landing `move-reparse-point-safety` first, or in the same batch, is recommended. There is no hard dependency.
