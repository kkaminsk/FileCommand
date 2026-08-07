# M4 — Viewer & external editor

## Why

After M3 the panels, command line, and menus work, but there is still no way to inspect file contents in place, and F3/F4 are dead keys. Users need a fast read-only viewer that opens multi-GB files instantly (log files, dumps, binaries) and a way to edit files before the built-in editor exists. Shipping the F4 external-editor hook now makes editing useful immediately while deferring the heavier built-in editor to M5.

## What Changes

- Add the F3 built-in read-only viewer with two modes:
  - **Text mode** — UTF-8 decoding with lossy fallback, F2 wrap/unwrap toggle, F7 incremental search, and header indicators for column/offset, total size, and byte-offset percent.
  - **Hex mode** — classic `offset | hex bytes | ASCII gutter` layout, entered/left via the F4-in-viewer mode toggle (label swaps `Hex`/`ASCII`).
- Open files of any size instantly by memory-mapping / chunk-reading with **no full line index**: percent is byte-offset-based; backward navigation scans backward for line starts under a max-line-length cap (e.g. 64 KB) that hard-splits over-long lines; search streams with overlap at chunk boundaries; hex layout is pure offset math.
- Render the viewer as a frame-less full-screen view with its own F-key bar: `1Help 2Unwrap 4Hex 7Search 10Quit`.
- Add the F4 external-editor hook: read an editor command from `config.toml`, suspend the TUI, launch the editor on the file under the cursor, and restore the TUI on exit. The panel re-reads on return.

## Capabilities

### New Capabilities

- `viewer`: The F3 read-only viewer with text and hex modes (F4-in-viewer toggle, F2 wrap/unwrap, F7 chunk-overlapped search, byte-offset percent and col/offset/size header indicators) that opens multi-GB files instantly via mmap/chunk reads with no full line index, plus the viewer F-key bar.
- `external-editor`: The F4 hook that reads an editor command from config, suspends the TUI, launches the external editor on the current file, and restores the TUI on exit — shipping before the built-in editor lands in M5.

### Modified Capabilities

- None (greenfield project; no existing specs)

## Impact

- **`filecommand-core`**
  - New `viewer` module: streaming/mmap file access, decode-with-lossy-fallback, line-start backward scan with max-line-length cap, chunk-overlapped search, hex offset math, viewer state machine (mode, wrap, scroll offset, search state) driven through `core::update`.
  - `config` module: add/consume the `editor =` external-editor command key (already reserved in the schema, §6).
  - `shell`-style suspend/restore seam reused for launching the external editor (spawn in the panel's directory, suspend TUI while running).
- **`filecommand-tui`**
  - New `views/viewer` renderer (text + hex bodies, header row, viewer F-key bar) in the frame-less full-screen layout (§4.5).
  - `input/` routing for the viewer focus (F2/F4/F7/F10 and navigation) and the F4-from-panel external-editor path.
- **Dependencies:** add a memory-map crate (e.g. `memmap2`) for `filecommand-core`; reuse existing `unicode-width` for column/display width; `insta` + ratatui `TestBackend` for viewer text/hex snapshot tests.
- **Config/docs:** `config.toml` `editor =` documented; Help "Viewer" topic content authored in M5 but the viewer keys are stable now.
