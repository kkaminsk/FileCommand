# Tasks: link-aware-listing

## 1. Data model (`crates/filecommand-core/src/listing.rs`)

- [x] 1.1 Add `link: bool` to `RawDirEntry` and `Entry`. `Entry::parent_dir()` sets `link: false`, and `Entry::from(RawDirEntry)` copies it (design D3)
- [x] 1.2 Add `RawDirListing { entries: Vec<RawDirEntry>, unreadable: usize }` and change `FsReader::read_dir` to return `io::Result<RawDirListing>` (design D6)
- [x] 1.3 Change `list_dir_chunked` to return `io::Result<ListingTotals { entries: usize, unreadable: usize }>`. `list_child_dirs` reads `.entries` (design D6)
- [x] 1.4 Update every `Entry { .. }` / `RawDirEntry { .. }` struct literal in core and tui sources and tests with `link: false`. If `streaming-listing-performance` has landed, add `link` to `Entry::new` instead, and run the classification inside `for_each_entry`. Also update the `FsReader` test doubles (`listing.rs` `FakeReader`/`TreeReader`, `filecommand-tui/src/worker.rs` `FakeReader`) to the new return type

## 2. Reader (`listing.rs`)

- [x] 2.1 Add a pure `classify_entry(name, own: io::Result<EntryFacts>, follow: impl FnOnce() -> io::Result<EntryFacts>) -> Option<RawDirEntry>` (design D8):
  - `own` is `Err` → `None` (counted as unreadable).
  - Not a link → as today.
  - A link with `follow` `Ok` → target's `is_dir` and size, the link's own `modified` and `hidden`, `link: true`.
  - A link with `follow` `Err` → `is_dir: false, size: 0, link: true` (design D1, D2).
- [x] 2.2 Rewrite `StdFsReader::read_dir` on top of it. `entry?` errors still return `Err`. `entry.metadata()` is adapted into `EntryFacts` (`is_symlink` from `file_type()`, `hidden` via the existing `is_hidden`). `follow` is `std::fs::metadata(long.join(name))`, and the count of `None` results becomes `unreadable` (panel-navigation: "One unreadable entry does not fail a listing"; design D6)
- [x] 2.3 In `find_in_subtree`, push a child onto the walk stack only when `raw.is_dir && !raw.link`. Matching by name is unchanged (design D4)

## 3. Reducer and worker

- [x] 3.1 `update.rs`: add `unreadable: usize` to `Command::ListingComplete`. In `apply_listing_event`, after the existing completion work, set `last_error` to `1 entry could not be read` or `N entries could not be read` (N via `format_count`) when `unreadable > 0`. Update every existing `ListingComplete { .. }` construction in tests (design D7)
- [x] 3.2 `update.rs` `enter_delete_confirm_for_sources`: build the active panel's set of `link` entry names and exclude sources that are listed links of that panel from `needs_second_confirm` (design D5)
- [x] 3.3 `filecommand-tui/src/worker.rs`: `spawn_listing` sends `ListingComplete { panel, total: total + totals.entries, unreadable: totals.unreadable }`, and `gather_info` counts over `.entries` (design D6)

## 4. Tests

- [x] 4.1 `listing.rs`: `classify_entry` unit tests for each branch: unreadable, plain file, plain dir, link to a directory, link to a file (target size, link date), dangling link (design D8)
- [x] 4.2 `listing.rs`: `list_dir_chunked` with a `FakeReader` returning `unreadable: 2` reports `ListingTotals { entries, unreadable: 2 }`, and the `link` flag survives `Entry::from` and sorting (design D3, D6)
- [x] 4.3 `listing.rs`: `find_in_subtree` over a `TreeReader` fixture with a directory-link entry matches the link by name but never reads beneath it (design D4)
- [x] 4.4 `listing.rs` `#[cfg(windows)]`: in a temp directory, create a real directory, a file, a `mklink /J` junction to the directory, and a dangling junction (junction to a directory that is then removed). `StdFsReader::read_dir` reports the junction with `is_dir: true, link: true`, the dangling junction with `is_dir: false, link: true`, `unreadable == 0`, and does not fail (panel-navigation: "A junction to a directory lists as a directory", "A dangling link does not fail the listing")
- [x] 4.5 `listing.rs` `#[cfg(unix)]`: `std::os::unix::fs::symlink` to a directory, to a 4,096-byte file, and to a missing path, classified as directory, as a file of 4,096 bytes, and as a file respectively (panel-navigation: "A directory symbolic link behaves like a junction", "A link to a file lists as a file with the target's size")
- [x] 4.6 `update/tests.rs`: Enter on a `Directory` entry with `link: true` emits `StartListing` for `cwd.join(name)`. Ctrl+PgUp from there lists the containing directory (panel-navigation: "Enter descends into a directory link", "Parent navigation from inside a link returns to the link's directory")
- [x] 4.7 `update/tests.rs`: Enter on a dangling link (a `File` entry with `link: true`) does not start a listing (panel-navigation: "A dangling link does not fail the listing")
- [x] 4.8 `update/tests.rs`: `ListingComplete { unreadable: 1 }` sets `last_error` to `1 entry could not be read`, and `unreadable: 1234` gives `1,234 entries could not be read`. A following re-read with `unreadable: 0` leaves `last_error` as `None` (panel-navigation: "One unreadable entry is skipped and reported", "The message clears on the next listing", "Completion after skipped entries shows the count")
- [x] 4.9 `update/tests.rs`: `ListingFailed` still sets the inline error, unchanged (panel-navigation: "A directory that cannot be enumerated still shows the read-error state")
- [x] 4.10 `update/tests.rs`: F8 on a directory-link entry opens the delete confirmation with `needs_second_confirm == false`. F8 on a real directory still requires the second confirmation (panel-navigation: "Deleting a directory link asks once"; design D5)
- [x] 4.11 `filecommand-tui/src/worker.rs` tests: `gather_info` with the new `FakeReader` shape counts directory links as directories
- [x] 4.12 Confirm no insta snapshot changes. Rendering is untouched

## 5. Verify

- [x] 5.1 `cargo test --workspace` green on Windows, including the `#[cfg(windows)]` junction test
- [x] 5.2 `cargo clippy --workspace --all-targets` introduces no new warnings
- [x] 5.3 `openspec validate link-aware-listing --strict` clean
- [ ] 5.4 Manual (Windows):
  - In a scratch directory run `mklink /J lnk <some dir>`. Confirm `lnk` shows `▶SUB-DIR◀`, Enter enters it, and Ctrl+PgUp returns.
  - Remove the target, Ctrl+R, and confirm `lnk` lists as a file and the listing does not fail.
  - F8 on a live junction asks once. If `move-reparse-point-safety` has landed, it removes only the link and leaves the target intact. Before that change, the Delete job itself fails with "Access is denied", a pre-existing `RealFs` defect that change fixes.
