# Design: link-aware-listing

## Context

All of the following was verified on `build/open-proposals-2026-10` @ `1bada6f`, and the `std::fs` behavior with probes compiled by the audited toolchain (`rustc 1.98.1`) on this Windows 11 machine.

**The reader.** `StdFsReader::read_dir` (`listing.rs`) enumerates `std::fs::read_dir(to_long_path(path))` and, for every entry, runs `let entry = entry?; let metadata = entry.metadata()?;`. It then builds `RawDirEntry { name, is_dir: metadata.is_dir(), size, modified, hidden }`, where `hidden` comes from the same metadata's attribute bits (hidden-entry-styling D1: "never a per-file stat"). `Entry::from(RawDirEntry)` maps `is_dir` to `EntryKind::Directory` or `File`.

Four consumers share the reader:

- `list_dir_chunked`, the panel listing, driven by `filecommand-tui`'s `spawn_listing`, which sends `ListingChunk` and `ListingComplete { panel, total }`, or `ListingFailed { panel, message }` on any `Err`
- `list_child_dirs` (Tree mode)
- `find_in_subtree` (Alt+F7), which descends into every `raw.is_dir`
- `gather_info` (Info mode counts)

**What `std` reports for links.** A `mklink /J` probe printed the following:

| Call | Result for the junction |
|---|---|
| `read_dir` entry `file_type()` / `metadata()` | `is_dir=false`, `is_symlink=true`, attributes `0x410` (DIRECTORY \| REPARSE_POINT) |
| `std::fs::metadata` (follow) | `is_dir=true` |
| `std::fs::metadata` on a dangling junction | `NotFound` |

Rust's Windows `is_symlink()` is true for every *name-surrogate* reparse tag, so it covers both junctions and symlinks. Other reparse points keep `is_symlink() == false` and are unaffected: OneDrive cloud placeholders already report `is_dir()` correctly, and App Execution Aliases are files. Unix `DirEntry::metadata` is `lstat`, so a symlink to a directory also reports `is_dir=false`.

**Where per-entry metadata can fail.** On Windows, `DirEntry::metadata()` is assembled from the enumeration's own find data. A probe that deleted an entry mid-iteration and then called `metadata()` on it got `Ok`. So the R-21 `entry.metadata()?` failure is a non-Windows concern: on Unix, `lstat` fails with `ENOENT` (raced deletion) or `EACCES` (a directory readable but not searchable). The `entry?` iterator error is different: it is an error from the enumeration itself (`FindNextFileW` or `readdir` failing). On Windows the iterator re-calls `FindNextFileW` after an error, so "skip and continue" there could repeat the error indefinitely.

**The mini-status.** `views/panel.rs::render_bottom_border` shows, in priority order:

1. a status override (Tree or Quick View)
2. `panel.last_error`
3. `Reading… N` while streaming
4. the selection summary or the cursored entry's details

`last_error` is the established inline-message surface. It carries failed listings, failed F3/F4/open-with launches and the editor's "too large" redirect, is cleared by `begin_new_listing` and by successful launches, and is stored per tab.

**Second delete confirmation.** `enter_delete_confirm_for_sources` sets `needs_second_confirm = sources.iter().any(|s| s.is_dir)`, and the second prompt reads "`<name>` contains a directory — delete its entire contents?". `SourceItem::is_dir` comes from `Entry::is_dir_like()`. Today a junction is a `File`, so it gets one prompt.

## Goals / Non-Goals

**Goals:**

- Junctions, directory symlinks and Unix directory symlinks list as directories and can be entered.
- Links to files list as files with their target's size, and dangling links list as non-enterable entries without failing the listing.
- One entry whose metadata cannot be read no longer fails the whole listing, and the user is told how many were skipped.
- Keep the link distinction available internally (`link` flag) for the consumers that need it.

**Non-Goals:**

- A visual link marker in the entry row. "Entry row rendering" is modified in flight by `hidden-entry-styling` and `full-mode-column-widths`. A marker can follow as its own change.
- Recreating or managing links, and any `fs_ops` reparse behavior (`move-reparse-point-safety`).
- The same per-entry pattern in `fs_ops`' job enumeration (`RealFs::read_dir`). See D6.
- Streaming the enumeration (`streaming-listing-performance`) and stale-listing guards (`listing-request-guard`).
- Showing the link's own date versus its target's date as a user choice. The link's own date is shown (D1).

## Decisions

### D1: Classify a link by following it once, and keep the link's own name, date and hidden flag

For each enumerated entry, the reader computes `link = metadata.file_type().is_symlink()` from the enumeration's own metadata, at no extra I/O.

- **Not a link:** classification is unchanged and costs no extra call.
- **A link:** the reader calls `std::fs::metadata(<long path of the entry>)` once. On success:
  - `is_dir` and, for a file target, `size` come from the target.
  - `modified` and `hidden` stay those of the link entry. That keeps "what is in this directory" accurate and preserves hidden-entry-styling's "flag from the enumeration's own metadata" rule.

Entering a directory link sets the panel's `cwd` to the link's own path (`cwd.join(name)`). The listing reads the target's contents through it, and `parent_path` is lexical, so `..` and Ctrl+PgUp return to the directory containing the link. This matches Explorer and `cmd`.

*Alternative considered:* on Windows, decide from the link's own `FILE_ATTRIBUTE_DIRECTORY` bit with zero I/O. Rejected: it cannot tell a dangling link from a live one, it would list a dangling junction as an enterable directory that then fails to read, and Unix has no equivalent.
*Alternative considered:* follow every entry. Rejected: it adds a stat per entry to every listing, and hidden-entry-styling D1 deliberately avoids per-file stats.

### D2: A link that cannot be followed lists as a non-enterable file entry

When following fails for any reason (`NotFound` for a dangling target, or access denied on the target), the entry becomes `RawDirEntry { is_dir: false, size: 0, link: true, .. }`, keeping its own date and hidden flag. Enter on it opens the file-action menu, as it does for every link today. The listing never fails because of it.

*Alternative considered:* omit dangling links. Rejected: the user could not see or delete the broken link.

### D3: `link: bool` on `RawDirEntry` and `Entry`, internal only

Both structs gain `link`. `Entry::parent_dir()` sets it to `false`, and `Entry::from(RawDirEntry)` copies it. Nothing renders it (non-goal). It has two consumers today, D4 and D5. Sorting, selection and every other `is_dir_like()` user treat a directory link exactly as a directory: F3 does nothing on it, F4 does not launch the editor, Quick View shows its directory indicator, Brief and Full render it as a directory, and drop-target validation treats it as a directory.

### D4: Find-file does not descend into links; Tree mode shows directory links

`find_in_subtree` pushes a child onto its walk stack only when `raw.is_dir && !raw.link`. It still *matches* a link by name. The rule keeps today's find-file results, since links were files, and avoids junction cycles. Windows ships such cycles: `C:\ProgramData\Application Data` points back to `C:\ProgramData`, and only an ACL stops traversal there today.

`list_child_dirs` (Tree) needs no special case. Directory links appear as children, and expansion is lazy and user-driven (additional-panel-modes "Tree lazy expansion"), so a cycle costs only what the user explicitly expands.

*Alternative considered:* a visited-identity set in `find_in_subtree`. Rejected because it needs per-directory identity queries for no user-facing benefit over "links are leaves," the rule `fs_ops` Delete already uses.

### D5: Deleting a directory link asks for one confirmation, not two

`enter_delete_confirm_for_sources` builds the set of the active panel's entry names whose `link` is true, which is usually empty. It then sets `needs_second_confirm` only for a source that `is_dir` and is not a listed link of that panel (`s.path.parent() == Some(&panel.cwd)` and its name is in the set).

The second prompt says "contains a directory — delete its entire contents?". Deleting a link removes only the link (the scenario "Delete removes the link, not the target" under file-operations requirement "Reparse-point (symlink/junction) semantics"), so the prompt would be false. One confirmation is also today's behavior for junctions. operation-dialogs "Delete confirmation dialog" asks for a second confirmation "before removing a non-empty directory," which deleting a link is not, so that requirement is not modified. A directory link does now *look* like a non-empty directory, though, so the ADDED "Directory links list as directories" requirement states the single confirmation explicitly, with its own scenario. `SourceItem` is not changed.

### D6: An unreadable entry is skipped and counted; enumeration errors still fail; `fs_ops` is out of scope

**Panel reader.** In `StdFsReader::read_dir`:

- A failing `entry.metadata()`, the entry's own non-followed metadata, increments `unreadable` and `continue`s.
- A failing `entry?` (the enumeration itself) still returns `Err`, which becomes `ListingFailed` and the existing inline read-error state. Continuing after an iterator error could loop on a persistent `FindNextFileW` failure, and an enumeration error does not identify a single entry.

`FsReader::read_dir` returns `RawDirListing { entries, unreadable }`. `list_dir_chunked` returns `ListingTotals { entries, unreadable }`, and `spawn_listing` sends `ListingComplete { panel, total, unreadable }`. `list_child_dirs`, `find_in_subtree` and `gather_info` read `.entries` and ignore the count.

*Alternative considered:* a placeholder entry per unreadable name. Rejected: there is no kind, size or date to show, and in the raced-deletion case the name no longer exists, so any operation on it would fail.

**Job enumeration (not changed).** `fs_ops`' `RealFs::read_dir` has the same `entry.metadata()?` pattern (R-21's second location). It is listed as a non-goal rather than routed through the error-recovery dialog, for three reasons:

- On Windows it cannot fail per entry: it uses the same find data, as verified above.
- Elsewhere, a failure turns that one directory into a single `SkippedItem` in the end-of-job summary. The user is told, and no data is lost: a move leaves the non-empty source directory in place.
- The walkers (`walk_copy`, `walk_delete`) read only `RawEntry::name` and re-stat every child through `fs.metadata`, whose per-child failure already becomes its own skipped item. The real fix is to drop the unused `RawEntry::metadata`, a `FileSystem` trait change that `durable-file-copy` and `move-reparse-point-safety` are reshaping in the same file.

Routing it through the error-recovery dialog would not fit. Enumeration runs in `plan_copy`/`walk_delete` before the job's per-file dialog loop exists, and every other walk-phase failure is reported as a skipped item, not a dialog. A small follow-up after those two changes land is the right vehicle.

### D7: The count is shown on the mini-status through `last_error`

On `ListingComplete` with `unreadable > 0`, the reducer sets the panel's `last_error` to `"1 entry could not be read"` or `"N entries could not be read"`, with N comma-grouped by `format_count`.

`last_error` is the existing inline-message surface. It needs no new state or rendering, so there is no snapshot churn, it is cleared by the next `begin_new_listing` of that panel (navigation, Ctrl+R, job re-read), and it is stashed per tab.

The message replaces the reverted name/size/date/time line for that listing, which the existing "Streaming listing mini-status" requirement would otherwise demand on every completion. That requirement is therefore MODIFIED: its description gains the exception, and the WHEN of its "Mini-status reverts on completion" scenario becomes "completes and no entry was skipped as unreadable". Its other three scenarios are unchanged, and one scenario for the exception is added. No other new or in-flight change modifies that requirement.

*Alternative considered:* a transient notice. Rejected: the only transient surface is the clipboard-specific `ClipboardFeedback`, which is cleared by any non-`Tick` command. A git-info or Info reply arriving right after `ListingComplete` could wipe it before it is seen.
*Alternative considered:* a count inside the `Reading… N` text. Rejected: it disappears at completion.

The trade-off: while the message stands, it hides the selection summary and the cursored entry's details, just as a failed F3 does today. A Ctrl+R that reads everything clears it.

### D8: Testability via a pure classification helper

The per-entry decision ("unreadable", "plain", "link → followed", "link → dangling") is a pure function over a small `EntryFacts { is_dir, is_symlink, len, modified, hidden }` value plus a follow closure returning `io::Result<EntryFacts>`. `StdFsReader` adapts `std::fs::Metadata` into it. Unit tests exercise every branch on any platform. Real-filesystem tests cover junctions on Windows (`#[cfg(windows)]`, `mklink /J`, including a dangling one made by deleting the target) and symlinks on Unix (`#[cfg(unix)]`, `std::os::unix::fs::symlink`).

## Risks / Trade-offs

- [A link to an unreachable network target stalls the listing for the SMB timeout while it is followed] → Only link entries are followed. The read runs on the listing worker, so the UI stays interactive, and `listing-request-guard` makes a superseded listing harmless.
- [Hidden or system profile junctions (`Application Data`, `My Documents`) become enterable directories whose listing fails with "Access is denied"] → This is correct and matches Explorer and `cmd`. The panel shows its existing inline read-error state, and `..` leaves.
- [`last_error` hides the selection summary while the unreadable-count message stands] → Rare in practice (non-Windows races and unsearchable directories), and cleared by the next listing. Documented in D7.
- [Struct-literal churn] Every `Entry { .. }` and `RawDirEntry { .. }` literal in tests gains `link: false`. → Mechanical. `hidden-entry-styling` set the precedent.
- [Enter on a junction cycle (for example `ProgramData\Application Data`) keeps nesting the path] → User-driven only, one level per Enter, and blocked by the ACL on stock Windows. find-file never follows (D4).

## Open Questions

None.
