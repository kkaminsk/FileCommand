# Tasks: viewer-mapping-safety

## 1. Byte source (`crates/filecommand-core/src/viewer/byte_source.rs`)

- [ ] 1.1 Replace `enum Backend` with a plain `file: File` field on `ByteSource`, and route `read_range` through `positioned_read_to_eof` unconditionally (viewer: "Instant open of arbitrarily large files"; design D1)
- [ ] 1.2 Open through `OpenOptions::new().read(true)` and, under `cfg(windows)`, `OpenOptionsExt::share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)`, using named constants with a comment that cites the spec's sharing guarantee (design D2)
- [ ] 1.3 Replace the `cfg(not(any(windows, unix)))` `Ok(0)` stub with a seek-then-read fallback on `&File`. Comment that a `ByteSource` is used by one thread at a time (design D4)
- [ ] 1.4 Remove `open_chunked` and the `memmap2` import, and rewrite the module and `open` doc comments to describe positioned reads, the sharing mode, and the shrink-reads-short rule (design D1, D3)
- [ ] 1.5 Confirm that `read_range` still maps an I/O error to an empty window and truncates to the bytes actually read, so a truncated file reads short (design D3)

## 2. Dependencies and call sites

- [ ] 2.1 Remove `memmap2` from `[workspace.dependencies]` in `Cargo.toml` and from `crates/filecommand-core/Cargo.toml`, then refresh `Cargo.lock` (proposal: What Changes)
- [ ] 2.2 `crates/filecommand-tui/src/worker.rs` `spawn_viewer_search`: update the doc comment ("a fresh mmap open"), and make sure the `ByteSource` is a local that drops when the search returns or is cancelled (design D5)
- [ ] 2.3 `crates/filecommand-tui/src/app.rs` `Effect::OpenViewer` arm: change the "opening/mapping" comment to describe a positioned-read open. No behavior change (design D5)
- [ ] 2.4 Quick View (`crates/filecommand-tui/src/views/panel.rs` `render_quick_view_body`) opens a `ByteSource` per frame. Confirm it needs no code change and that, after 1.1, it no longer maps the previewed file. Its snapshots are covered by 3.8 (design D1, "Scope: every mapping goes")
- [ ] 2.5 Update the remaining memory-map wording: the `crates/filecommand-tui/src/views/viewer.rs` module doc ("into a memory map (or a small positioned read …)") and `README.md` § Viewer (F3) and editor (F4) ("streams/memory-maps"). The original design doc (`docs/superpowers/specs/2026-08-06-filecommand-design.md` §4.5) is historical and stays as it is; the spec supersedes it

## 3. Tests

- [ ] 3.1 `byte_source.rs`: append through a second handle while a `ByteSource` is open. The write succeeds, and `read_range` stays clamped to the snapshot length (viewer: "Another program keeps writing to the viewed file")
- [ ] 3.2 `byte_source.rs`: truncate a 10 KiB file to 1 KiB with `set_len` while it is open. The truncation succeeds (on Windows this fails today with OS error 1224). `read_range(5000, 100)` is empty, `read_range(0, 2000)` returns 1024 bytes, and nothing panics (viewer: "Truncation while viewing never crashes", "Viewed file can be truncated, rotated, or deleted")
- [ ] 3.3 `byte_source.rs`: `std::fs::rename` and `std::fs::remove_file` on the file succeed while a `ByteSource` is open (viewer: "Viewed file can be truncated, rotated, or deleted")
- [ ] 3.4 `forward.rs`, `backward.rs`, `search.rs`: after truncation, `next_line_start`, `previous_line_start`, and `find_forward` terminate and return in-range offsets (design D3)
- [ ] 3.5 `crates/filecommand-tui/src/app.rs` tests: after `Effect::OpenViewer` then `Command::ViewerClose`, `rt.viewer_source` is `None` and the file can be removed together with its directory (viewer: "Closing the viewer releases the file")
- [ ] 3.6 With `listing-request-guard`'s viewer-search cancellation in place: start a search on a large file, close the viewer, and assert that the worker thread finishes and the file and its directory can then be removed (viewer: "Closing the viewer releases the file"; design D5)
- [ ] 3.7 Remove `chunked_fallback_serves_identical_bytes_to_the_mmap_path` and `chunked_open_offsets_are_also_clamped_to_snapshot_length`, and keep the empty-file, clamp, and missing-path tests passing through `open` (viewer: "Network and special files are read the same way")
- [ ] 3.8 Confirm that the viewer and Quick View insta snapshots are unchanged (design: Migration Plan)

## 4. Verify

- [ ] 4.1 `cargo test --workspace` green on Windows
- [ ] 4.2 `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] 4.3 `cargo tree -p filecommand-core` no longer lists `memmap2`, and `git grep -n "unsafe" crates/filecommand-core/src/viewer` finds nothing
- [ ] 4.4 Manual: view a log that another process appends to and truncates (for example a PowerShell loop calling `Add-Content`, then `Clear-Content`). The writer never fails, and FileCommand keeps running
- [ ] 4.5 `openspec validate viewer-mapping-safety --strict` clean
