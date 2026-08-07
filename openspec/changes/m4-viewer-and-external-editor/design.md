# Design — M4 Viewer & external editor

## Context

M4 adds the first read-only content view (F3) and the first editing affordance (F4 external editor) on top of the M1–M3 shell. The central engineering constraint comes from §4.5: the viewer "must open multi-GB files instantly" and "builds no full line index." That single requirement — instant open, bounded memory, no whole-file scan — drives every decision below. The viewer logic lives in `filecommand-core` (§3.1) so it is unit-testable without a terminal, and the TUI (§3.2) only renders the resulting view state, consistent with the "rendering is a pure function of core state" rule (§3.3). The external-editor hook reuses the TUI-suspend/restore seam already built for `shell` command passthrough in M3 (§3.1 `shell`), so M4 adds behavior, not a new subsystem, for F4.

## Goals / Non-Goals

**Goals:**

- Open any file (including multi-GB) in the F3 viewer with first paint independent of file size — no full read, no line index, bounded working-set memory.
- Correct byte-accurate hex mode and lossy-but-stable UTF-8 text mode, with F2 wrap/unwrap, F7 search, and the col/offset/size/percent header indicators of §4.5.
- Backward navigation and search that work under the streaming model: backward line-start scan with a max-line-length cap that hard-splits, and search with chunk-boundary overlap so no match is missed across chunk seams.
- A frame-less full-screen viewer view and its F-key bar (`1Help 2Unwrap 4Hex 7Search 10Quit`), rendered per the §4.11 theme roles (`viewer.header`, `viewer.text`, `viewer.match`).
- F4 external-editor hook: config-driven command, TUI suspend, launch on the current file in the panel's directory, restore + panel re-read on exit.

**Non-Goals:**

- The built-in editor (F4 built-in) — deferred to M5 (§4.6). M4 ships only the external hook.
- Quick View panel mode (§4.2) — separate capability in M5, though it reuses the same text-head rendering idiom.
- Editable content, search-and-replace, regex, or writing of any kind in the viewer (read-only, §4.5).
- Syntax highlighting, encoding auto-detection beyond UTF-8-with-lossy-fallback, or BOM/encoding menus.
- Truecolor-specific viewer styling beyond the mandatory ANSI-16 roles (§4.11 color-depth policy).

## Decisions

### D1 — Viewer lives in `filecommand-core`, driven through `core::update`

Per §3.1/§3.3 the viewer's state machine (mode, wrap on/off, current byte offset / top-of-screen anchor, search pattern and match cursor) lives in core with no ratatui/crossterm dependency, and all transitions flow through `core::update`. The TUI `views/viewer` renderer (§3.2) is a pure function of that state and a window into the file bytes. This keeps the hard logic (backward scan, chunk-overlap search, hex offset math) unit-testable against temp files without a terminal, matching the §8 testing strategy, and lets snapshot tests pin the rendered frames.
*Alternative rejected:* implementing the viewer entirely in the TUI crate — would push file-access and scan logic behind the terminal boundary and make it untestable in isolation, violating the crate split.

### D2 — Memory-map with chunk-read fallback; no full line index

Adopt §4.5 literally: memory-map the file (add `memmap2` to `filecommand-core`) and treat it as a byte window, falling back to positioned chunk reads when mmap is unavailable (e.g. certain network paths or zero-length special files). Open cost is O(1) — map the file, read only the bytes needed for the visible screen. **No line index is ever built.** Consequences, all per §4.5:
- **Percent indicator** is byte-offset-based: `top_offset / file_len`, not line-based.
- **Forward paging** decodes bytes from the current top offset for the number of visible rows.
- **Hex mode** is pure offset math: row `r` shows bytes `[base + r*16, base + r*16 + 16)`; no state beyond the base offset.

*Alternative rejected:* building a line-offset index on open (fast random line access) — an index over a multi-GB file is exactly the whole-file scan §4.5 forbids and would break the "instant open" goal (§2, §4.5).

### D3 — Backward navigation by bounded backward scan with a hard-split cap

Because there is no line index, moving up / paging up cannot look up a previous line offset. Scroll-up scans backward from the current top offset for the previous line-start byte (`\n`), capped at a **max line length (e.g. 64 KB)**; if no newline is found within the cap, the line is **hard-split** at the cap boundary and that split point becomes the synthetic line start. This bounds the backward read per keystroke to the cap regardless of file content (e.g. a multi-GB file with no newlines), keeping upward navigation responsive and memory-bounded.
*Alternative rejected:* unbounded backward scan to the true line start — a single newline-free file would scan gigabytes per up-arrow.

### D4 — Search streams forward with chunk-boundary overlap

F7 search reads the file in fixed chunks starting from the current offset and scans for the pattern, carrying an **overlap of `pattern_len - 1` bytes** across each chunk boundary so a match straddling two chunks is still found (§4.5 "search streams with overlap at chunk boundaries"). Search runs bounded per step and reports the next match offset, which becomes the new top anchor; the match cell is styled `viewer.match` (§4.11). Search is byte/substring oriented (literal), consistent with the "no regex" scope discipline the spec applies to the editor (§4.6) and with keeping search allocation-free over huge files.
*Alternative rejected:* loading the file into a string and using `str::find` — impossible for multi-GB files and defeats the streaming model.

### D5 — Text decode is UTF-8 with lossy fallback, display-width aware

Text mode decodes the visible byte window as UTF-8 and substitutes replacement characters for invalid sequences (§4.5 "UTF-8 with lossy fallback"), reusing the `listing` module's established rendering discipline (§3.1): control and zero-width characters are replaced, and column layout uses `unicode-width` grapheme/display width so CJK/emoji and the col indicator stay aligned. F2 toggles wrap: unwrap renders each logical line clipped to the viewport with horizontal offset (the header `Col` indicator reflects the horizontal scroll); wrap re-flows at the viewport width. Only the visible window is decoded — decoding never runs over the whole file.

### D6 — F4-in-viewer toggles mode; frame-less full-screen chrome

The viewer is a full-screen frame-less view replacing the panels (§4.5), with a header row (`viewer.header`) carrying the col/offset, size, and percent indicators, and the viewer F-key bar `1Help 2Unwrap 4Hex 7Search 10Quit` (`keybar.number`/`keybar.label` roles). F4 inside the viewer toggles text↔hex, with the label swapping `Hex`/`ASCII` exactly as the reference screenshots show (§4.5). Input routing (§3.2 `input/`) gives the viewer its own focus target so F2/F4/F7/F10 and navigation keys are handled by the viewer, not the panels, while it is open.

### D7 — F4 external editor reuses the shell suspend/restore seam

The F4-from-panel external-editor hook reads the `editor =` command from `config.toml` (§6; the key is already reserved in the `config` schema, §3.1) and launches it on the file under the cursor using the same TUI-suspend/restore mechanism `shell` uses for command passthrough (§3.1 `shell`, §4.2 command line): leave raw mode / alternate screen, spawn the editor as a child in the **panel's current directory**, wait, then restore the terminal and re-read the panel (§7 refresh policy — panels re-read after our own operations). On Windows the command is spawned the same way as shell commands (default `cmd.exe /C` semantics from §3.1) so an editor like `notepad`/`code` launches with correct working directory and long-path handling via the `\\?\` abstraction (§3.1 `fs_ops`). If `editor =` is unset, F4 shows a message dialog (built-in editor arrives in M5); this keeps F4 useful now without blocking on M5.
*Alternative rejected:* building a second process-launch path specific to the editor — needless duplication of the suspend/restore and long-path handling already centralized for `shell`.

### D8 — Testing via injected seams and `TestBackend` snapshots

Core viewer logic is tested against temp files through the same fs trait seam used elsewhere (§8): decode/lossy handling on non-UTF-8 bytes, backward-scan hard-split on a newline-free block, chunk-overlap search finding a boundary-straddling match, and byte-offset percent math. The TUI renders viewer text mode, hex mode, and the viewer F-key bar via ratatui `TestBackend` + `insta` snapshots (§8 explicitly lists "viewer text/hex"), with time pinned through the injected `Clock` trait and terminal size/locale fixed so frames are deterministic.

## Risks / Trade-offs

- **mmap on a file that is truncated/grows while open can fault or show stale bytes** -> Treat the mapped length as a snapshot taken at open; clamp all offsets to that length; fall back to positioned chunk reads for paths where mapping fails, and surface an inline error rather than panicking (§7 panic/error policy).
- **A multi-GB file with no newlines could make naive backward scan or search unbounded** -> The max-line-length cap (D3) hard-splits and the chunked search (D4) bound every keystroke's work to a fixed window, independent of file content.
- **Chunk-boundary search could miss a match straddling two chunks** -> Carry a `pattern_len - 1` byte overlap between consecutive chunks (D4); covered by an explicit boundary-straddle unit test.
- **Lossy UTF-8 decoding of a partial multi-byte sequence at the visible window's edge could show spurious replacement chars** -> Decode from a byte window slightly larger than the viewport and align the window to a decode boundary where possible; the col/percent indicators remain byte-based so they stay correct regardless.
- **External editor may not exist, may block, or may leave the terminal dirty on crash** -> Validate/spawn like a shell command; the panic/exit path restores raw mode + alternate screen (§7) so a misbehaving editor cannot leave FileCommand's terminal corrupted; re-read the panel on return so on-disk edits are reflected.
- **F4 semantics change between M4 (external) and M5 (built-in)** -> Behavior is config-gated on `editor =`: set → external; unset → M4 shows a "no editor configured" message, M5 opens the built-in editor. The key binding and dialog copy are stable across the transition.

## Open Questions

- Exact default chunk size and the max-line-length cap value (§4.5 suggests 64 KB) — to be fixed during implementation against the 100k-entry / multi-GB manual test targets (§8), not blocking on the design.
- Whether unwrap horizontal scrolling exposes a `Col` value counted in display columns vs. byte columns for lines containing wide characters — leaning display columns to match the alignment rule (D5); to confirm with a snapshot fixture.
