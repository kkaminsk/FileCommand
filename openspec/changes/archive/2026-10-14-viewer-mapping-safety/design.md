# Design: viewer-mapping-safety

## Context

Verified on `build/open-proposals-2026-10` (`1bada6f`).

**The byte source.** `crates/filecommand-core/src/viewer/byte_source.rs` defines `enum Backend { Mmap(memmap2::Mmap), File(File) }` and `ByteSource { len, backend }`.
- `ByteSource::open` calls `File::open`, snapshots `metadata().len()`, and then tries `unsafe { memmap2::Mmap::map(&file) }`. It falls back to `Backend::File` when mapping fails. Mapping fails for a zero-length file, and for some network or special paths.
- `open_chunked` forces the `File` backend. It exists only so a test can compare the two backends.
- `read_range(offset, len)` clamps to the snapshot length, then:
  - Mmap: copies `mmap[offset..offset+n].to_vec()`.
  - File: allocates `vec![0; n]` and fills it with `positioned_read_to_eof`, a loop over `seek_read` (Windows) or `read_at` (Unix) that handles short reads. It maps an I/O error to an empty result (`unwrap_or(0)`) and truncates the buffer to the bytes actually read.
- On targets that are neither Windows nor Unix, `positioned_read` is a stub returning `Ok(0)`. An unmappable file then renders as empty without any error.

**Who reads, and how much.** Every consumer reads a bounded window and works on the returned `Vec`:
- The text renderer reads `DEFAULT_CHUNK_SIZE` (64 KiB) plus a UTF-8 margin per frame (`views/viewer.rs` `read_text_window`).
- Line navigation reads up to 64 KiB per step (`forward::next_line_start`, `backward::previous_line_start`).
- F7 search (`search::find_forward`) streams 64 KiB windows with overlap.
- Quick View (`views/panel.rs:883`, `render_quick_view_body`) opens a fresh `ByteSource` on the previewed file on every frame it draws.

The mmap path never avoids the copy, so the only thing it saves compared with the fallback is one `ReadFile`/`pread` call per window.

**Lifetimes.** The viewer's source lives in `Runtime::viewer_source` (`app.rs:61`). It is set on `Effect::OpenViewer` (`app.rs:926-937`) and cleared on `Command::ViewerClose` (`app.rs:801-803`). `spawn_viewer_search` (`worker.rs:133-145`) opens its own `ByteSource` and keeps it until `find_forward` returns. That can take a full pass over a multi-GB file, and the search cannot be cancelled today (audit P-10).

**Sharing.** `std::fs::File::open` on Windows already requests `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`, so the *handle* does not block other programs. What blocks them on Windows is the *mapping*: a file with a mapped view cannot be truncated (`ERROR_USER_MAPPED_FILE`). A review probe on this machine confirmed both halves: with a read-only mapped view open, `SetLength` from a second handle failed with error 1224, and with only a shared read handle open, the same call succeeded. On Unix nothing blocks the writer, and the mapping instead turns a truncation into `SIGBUS` on the next access to a page past the new end.

**Spec.** `openspec/specs/viewer/spec.md`, "Instant open of arbitrarily large files", says the viewer "MUST memory-map the file (falling back to positioned chunk reads when mapping is unavailable)". Its scenarios refer to "the mmap path" and "the mapped snapshot". No in-flight change has a delta on the `viewer` capability. The MODIFIED requirement changes the existing text in three places, all because the mapping is gone:

- The description's "mapped length" becomes "file length".
- The scenario "Mapping unavailable falls back to chunk reads" describes a fallback that no longer exists. It is replaced by "Network and special files are read the same way", which keeps its intent: network and special paths do not panic, and the rendered content is identical to any other read of the same byte range.
- The THEN of "Offsets clamped to the snapshot length" says "does not read past the snapshot length" instead of "the mapped snapshot". Its WHEN is unchanged.

## Goals / Non-Goals

**Goals:**

- No `unsafe` code in the viewer, and no way for another process to crash FileCommand by changing the file it shows.
- While the viewer is open, other programs can keep appending to, truncating, deleting, and renaming the file on Windows.
- Keep the existing contract: O(1) open, bounded reads, the snapshot length, clamped offsets, and identical rendering.
- Release the file promptly when the viewer closes, including from a search worker.

**Non-Goals:**

- Live "tail" behavior, where the viewer would notice that the file grew and extend its snapshot. The snapshot stays fixed at open, as the spec already requires.
- Making F7 search cancellable. That is `listing-request-guard` (P-10). This change only depends on it (D5).
- Changing the viewer's lifetime across a terminal resize. That is `resize-preserves-session-state` (R-03, its D7).
- Reducing Quick View's open-per-frame cost. It stays as it is. It is now a plain open and a read, with no map and unmap.
- The `unsafe` FFI elsewhere in core (`drives.rs`, `fs_ops/fs.rs`). That is `ci-and-build-tooling` (B-07).

## Decisions

### D1: Positioned reads on every platform; drop the mmap backend and `memmap2`

`ByteSource` becomes `{ len: u64, file: File }`, and `read_range` always uses `positioned_read_to_eof`.

Rationale:
- Every read already copies into a `Vec`, so mapping saves no memory traffic.
- All reads are bounded (at most about 64 KiB plus margin per call), so the extra cost is one system call per window. The data comes from the OS page cache, the same place a page fault would have taken it from.
- Sequential F7 scans benefit from the OS read-ahead that positioned reads trigger.
- Removing the mapping removes both platform hazards at their source: there is no `SIGBUS` on Unix and no truncation lock on Windows.
- It also deletes the only `unsafe` block in the viewer, and one dependency.

**Scope: every mapping goes.** The F3 viewer, the F7 search worker and Quick View all obtain their bytes through `ByteSource::open`, so removing the backend removes all three mappings. That includes Quick View's map-and-unmap on every frame (`render_quick_view_body`), which today briefly pins the previewed file on Windows each frame and could fault on Unix if the file is truncated mid-frame. Quick View needs no code change of its own.

*Alternative considered: keep mmap on Windows only and use positioned reads on Unix.* This is safe on Windows in the memory-safety sense, but it keeps the user-visible Windows hazard: a viewed log cannot be truncated or rotated by its writer. It also keeps two code paths and an `unsafe` block with a platform-specific SAFETY argument. No reason to keep it was found. Speed is not one, because both paths copy. **Rejected.**

*Alternative considered: keep mmap and add a `SIGBUS` handler on Unix.* A signal handler that recovers from a fault inside safe Rust code is itself unsound and hard to test. **Rejected.**

*Alternative considered: use mmap only for files that have not been modified recently.* This is the audit's suggestion for non-Windows systems. A heuristic still crashes when the guess is wrong, and it does nothing for the Windows rotation problem. **Rejected.**

### D2: Explicit Windows share mode on open

`ByteSource::open` opens through `OpenOptions::new().read(true)` plus `std::os::windows::fs::OpenOptionsExt::share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)` (the values `0x1 | 0x2 | 0x4`, declared as named constants beside the call). The standard library's default happens to be the same today. Writing it out makes the guarantee in the spec visible in the code, and it can be tested (D6) instead of being an unstated property of `std`.

### D3: The snapshot rule is unchanged; shrinking files read short, never fault

`len` is still captured once at open, and every read is still clamped to it. If the file shrinks while it is open, `positioned_read_to_eof` stops at the real end of file (`Ok(0)`) and `read_range` returns the bytes that exist, possibly none.

The existing consumers already handle short and empty windows. Verified:
- `next_line_start` treats a short window as end of file.
- `previous_line_start` returns `scan_start` for an empty window.
- `find_forward` stops when a chunk comes back shorter than requested.
- `scroll_lines` and `scroll_to_end` stop once a scan makes no progress.

So a truncated file shows blank rows past its new end, and navigation stays bounded. A file that grows is shown only up to the snapshot length, exactly as today. I/O errors keep today's behavior: they are mapped to an empty window, never to a panic.

### D4: Replace the `Ok(0)` stub with a seek-and-read fallback

On targets that are neither Windows nor Unix, `positioned_read` seeks the handle (`(&file).seek(SeekFrom::Start(offset))`) and then reads. `&File` implements `Read` and `Seek`, so `read_range(&self)` keeps its signature. A `ByteSource` is used by one thread at a time: the render loop owns the viewer's source, and the search worker opens its own. The shared cursor therefore cannot race. A comment records this so a future change that shares a source across threads has to revisit it.

*Alternative considered: `compile_error!` on other targets.* Cross-platform builds are best-effort in this project, and a working slow path is better than no build. **Rejected.**

### D5: Releasing the file on close

- **Viewer.** No change. `apply` drops `rt.viewer_source` on `Command::ViewerClose`. After `resize-preserves-session-state`, it also drops it whenever the phase stops being `Viewer`. Because the handle shares delete, the file can be deleted even while it is open on Windows; releasing on close is what lets the directory that contains it be removed afterwards.
- **F7 search worker.** The worker's `ByteSource` is a local inside the spawned closure, so it is dropped when `find_forward` returns. `listing-request-guard` (P-10) passes a cancel flag into `find_forward` and sets it when the viewer closes or the search is superseded. Together they meet "search workers release the file when the viewer closes". This change adds the regression test (task 3.4) and does not add its own cancellation mechanism. Two mechanisms for the same flag would conflict, which is why the proposal asks to land after `listing-request-guard`.
- **Quick View.** Its source is a local in `render_quick_view_body` and is dropped at the end of each frame.

### D6: Tests exercise the platform guarantees through real files

These are `byte_source.rs` unit tests on temporary files. Each opens a `ByteSource`, then uses a second handle to:
- append, and assert that the append succeeds and that reads stay clamped to the snapshot length;
- truncate (`set_len`), and assert that the truncation succeeds and that reads past the new end come back short or empty without panicking;
- delete (`remove_file`) and rename the file, and assert that both succeed while the source is open.

On Windows the truncate test fails against today's mmap backend with OS error 1224 (`ERROR_USER_MAPPED_FILE`), so it is a genuine regression test. On Unix the same test pins "no SIGBUS". The mmap-versus-chunked parity test is removed together with `open_chunked`.

## Risks / Trade-offs

- [Throughput on very fast scrolling or wheel bursts is about one system call per 64 KiB window, compared with a page-cache memcpy.] → The copy already happens on both paths. Wheel input is coalesced (`app.rs:454-523`), so a burst is still a bounded number of reads per frame.
- [A truncated file shows blank rows up to the old length until the viewer is reopened.] → This is the documented snapshot rule. A follow-up could offer re-reading the length, which is out of scope here.
- [On Windows with POSIX delete semantics, a deleted file's name disappears at once while the viewer still shows its contents.] → This is the intended effect of sharing delete. The viewer shows what it opened.
- [Removing `open_chunked` changes a `pub` API.] → Its only callers are tests in the same module.

## Migration Plan

None for users. Code: remove `memmap2` from both manifests and refresh `Cargo.lock`. No config, data, or snapshot changes are expected, because the rendered bytes are identical.

## Open Questions

None.
