# Change: viewer-mapping-safety

## Why

Audit finding **R-20 (Low)**: the F3 viewer memory-maps the file it shows (`unsafe { memmap2::Mmap::map(&file) }`, `crates/filecommand-core/src/viewer/byte_source.rs:37`). The `unsafe` block has no `// SAFETY:` comment. Its module doc says reads are safe "even if the on-disk file changes underneath us", but that is only partly true, and in a different way on each platform:

- **Linux and other Unix systems:** if another process truncates the file while it is mapped, the next access to a page past the new end raises `SIGBUS` and kills FileCommand. Viewing a live log that gets rotated by truncation can crash the app.
- **Windows:** the mapping cannot be invalidated, but it pins the file. While the viewer is open, the writer cannot truncate the file (`SetEndOfFile` fails with `ERROR_USER_MAPPED_FILE`), so a program that rotates its log by truncation fails for as long as the user looks at the log. Each F7 search opens a second mapping on its own worker thread (`crates/filecommand-tui/src/worker.rs:133-145`), and Quick View maps the previewed file again on every frame (`crates/filecommand-tui/src/views/panel.rs:883`).

The mapping gives nothing back for this risk. Every read already copies the bytes out (`mmap[..].to_vec()`), in windows of at most 64 KiB plus a small margin. The positioned-read fallback already in the same file does the same work with one system call per read.

## What Changes

- The viewer reads files only through bounded positioned reads on an open handle, on every platform. It no longer memory-maps them. The `Mmap` backend, the `unsafe` block, the `open_chunked` test-only constructor, and the `memmap2` dependency are removed. This covers every `ByteSource` user: the F3 viewer, its F7 search worker, and Quick View's per-frame preview.
- On Windows the viewer opens the file with `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`, stated explicitly in code rather than left to the standard library's default. Other programs can keep appending to, truncating, deleting, or renaming the file while it is shown.
- A file that shrinks while it is shown never crashes the app. Reads past the new end return only the bytes that still exist. Navigation stays clamped to the length captured at open, as it is today.
- On targets that are neither Windows nor Unix, the stub that returns `Ok(0)` (and so makes the viewer silently show nothing) is replaced by a seek-and-read fallback.
- The viewer, and any F7 search worker still scanning for it, release the file when the viewer closes.
- The `viewer` spec stops requiring memory mapping, and states the sharing and truncation guarantees.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `viewer`: "Instant open of arbitrarily large files". Memory mapping is replaced by positioned reads on every platform. The mmap-fallback scenario becomes a scenario that network and special files are read the same way. New scenarios cover concurrent writers, truncation while viewing, and releasing the file on close.

## Impact

- `crates/filecommand-core/src/viewer/byte_source.rs`: `Backend` enum removed, so `ByteSource` holds `{ len, file }`. `ByteSource::open` opens with explicit Windows share flags. `open_chunked` is removed. `read_range` always goes through `positioned_read_to_eof`. The `cfg(not(any(windows, unix)))` stub becomes a seek-then-read fallback. The module doc and tests are rewritten.
- `crates/filecommand-core/Cargo.toml` and the workspace `Cargo.toml`: `memmap2` is removed from `[dependencies]` and `[workspace.dependencies]`. `Cargo.lock` drops `memmap2`.
- `crates/filecommand-tui/src/worker.rs`: the `spawn_viewer_search` doc comment no longer mentions "a fresh mmap open". The worker's `ByteSource` is scoped to the search.
- `crates/filecommand-tui/src/app.rs`: the `Effect::OpenViewer` comment ("opening/mapping") is updated. No behavior change. The viewer source is still released on `Command::ViewerClose`.
- `crates/filecommand-tui/src/views/panel.rs` (`render_quick_view_body`): no code change. Quick View now opens without mapping.
- Tests in `byte_source.rs` (truncate, append, delete, and rename while open), plus a TUI test that closing the viewer drops the source.

**Audit findings:** R-20

**Coordination:**
- `resize-preserves-session-state` (R-03) keeps the viewer phase, and its `ByteSource`, alive while the terminal is undersized. It releases the source when the viewer phase ends (its D7). This change relies on that lifetime rule and does not change it. Either order works.
- `listing-request-guard` (R-04, P-10) makes F7 viewer searches cancellable. The "search worker releases the file when the viewer closes" scenario depends on that cancellation. **Land this change after `listing-request-guard`**, which is the natural order anyway (P-10 is part of a High-severity change; R-20 is Low).
- `ci-and-build-tooling` (B-03, B-04, B-07) lists every `unsafe` block and requires a `// SAFETY:` comment on each. This change deletes the `byte_source.rs:37` block instead of documenting it, and removes `memmap2` from the dependency list. If `ci-and-build-tooling` lands first, it has to give this block an interim SAFETY comment so its unsafe-documentation lint passes, and this change then deletes the block and the comment together. If this change lands first, that change has one block fewer to document.
