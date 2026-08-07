## ADDED Requirements

### Requirement: Instant open of arbitrarily large files

The viewer SHALL open any file, including multi-GB files, with first paint independent of file size. It MUST memory-map the file (falling back to positioned chunk reads when mapping is unavailable) and MUST NOT build a full line index or perform any whole-file scan on open. Only the bytes required for the visible screen SHALL be read to produce the first frame. The mapped length SHALL be treated as a snapshot taken at open, and all navigation offsets MUST be clamped to that length.

#### Scenario: First frame reads only the visible window

- **WHEN** F3 opens a multi-GB file
- **THEN** the viewer paints the first screen without reading the whole file and without constructing a line index
- **AND** only the byte range needed for the visible rows is read from the file

#### Scenario: Mapping unavailable falls back to chunk reads

- **WHEN** the file cannot be memory-mapped (e.g. a network path or special file)
- **THEN** the viewer serves the visible window via positioned chunk reads instead of panicking
- **AND** the rendered content is identical to the mmap path for the same byte range

#### Scenario: Offsets clamped to the snapshot length

- **WHEN** navigation would move the top anchor beyond the length captured at open
- **THEN** the top offset is clamped to the file length and the view does not read past the mapped snapshot

### Requirement: Text and hex modes with F4 mode toggle

The viewer SHALL provide a text mode and a hex mode. Text mode MUST decode the visible byte window as UTF-8 with lossy fallback, substituting the replacement character for invalid sequences, and MUST NOT decode the whole file. Hex mode SHALL render the classic `offset | hex bytes | ASCII gutter` layout computed by pure offset math, where row `r` shows the 16 bytes `[base + r*16, base + r*16 + 16)` with no state beyond the base offset. F4 pressed inside the viewer SHALL toggle between text and hex modes, and the corresponding F-key bar label MUST swap between `Hex` and `ASCII`.

#### Scenario: Lossy UTF-8 decode of invalid bytes

- **WHEN** the visible window contains bytes that are not valid UTF-8
- **THEN** text mode renders replacement characters in place of the invalid sequences and continues rendering the rest of the window

#### Scenario: Hex layout by offset math

- **WHEN** the viewer is in hex mode with a given base offset
- **THEN** each row displays its offset, the 16 bytes starting at `base + r*16`, and the ASCII gutter for those bytes
- **AND** no line index or per-row state beyond the base offset is used

#### Scenario: F4 toggles mode and label

- **WHEN** the user presses F4 while the viewer is in text mode
- **THEN** the viewer switches to hex mode and the F-key bar label reads `ASCII`
- **AND** pressing F4 again returns to text mode with the label reading `Hex`

### Requirement: F2 wrap and unwrap toggle

The viewer SHALL toggle line wrapping with F2. In unwrap mode each logical line MUST be clipped to the viewport with a horizontal offset, and the header `Col` indicator MUST reflect the horizontal scroll position. In wrap mode lines MUST be re-flowed at the viewport width. Only the visible window SHALL be decoded in either mode.

#### Scenario: Unwrap clips with horizontal scroll

- **WHEN** the viewer is in unwrap mode and the user scrolls horizontally on a long line
- **THEN** the line is clipped to the viewport at the current horizontal offset
- **AND** the header `Col` indicator reflects that horizontal offset

#### Scenario: Wrap re-flows at viewport width

- **WHEN** the user presses F2 to enable wrap
- **THEN** logical lines are re-flowed to the viewport width so no horizontal scrolling is required

### Requirement: Byte-offset header indicators

The viewer header SHALL display the current column/offset, the total file size, and a percent-through indicator. The percent indicator MUST be computed from byte offsets as `top_offset / file_len`, not from line numbers, and MUST remain correct regardless of decoding or wrap state.

#### Scenario: Percent is byte-offset based

- **WHEN** the top-of-screen anchor is at byte offset `top_offset` in a file of length `file_len`
- **THEN** the header percent indicator equals `top_offset / file_len`
- **AND** the value is unaffected by whether the file contains valid UTF-8 or how lines wrap

#### Scenario: Size indicator reflects the opened file

- **WHEN** the viewer is open on a file
- **THEN** the header shows that file's size in bytes alongside the col/offset and percent indicators

### Requirement: Bounded backward navigation with hard-split cap

Because no line index exists, upward navigation SHALL locate the previous line start by scanning backward from the current top offset for a newline, bounded by a maximum line length cap (e.g. 64 KB). If no newline is found within the cap, the line MUST be hard-split at the cap boundary and that split point becomes the synthetic line start. The backward read per keystroke MUST be bounded to the cap regardless of file content.

#### Scenario: Backward scan finds the previous line start

- **WHEN** the user scrolls up and a newline exists within the cap before the current top offset
- **THEN** the top anchor moves to the byte after that newline

#### Scenario: Newline-free content is hard-split at the cap

- **WHEN** the user scrolls up in a region containing no newline within the cap distance
- **THEN** the line is hard-split at the cap boundary and that boundary becomes the synthetic line start
- **AND** the backward read performed does not exceed the cap even in a multi-GB newline-free file

### Requirement: F7 streaming search with chunk-boundary overlap

F7 SHALL perform a literal (substring) search that streams the file forward in fixed chunks starting from the current offset. Consecutive chunks MUST carry an overlap of `pattern_len - 1` bytes so a match straddling a chunk boundary is still found. The next match offset SHALL become the new top anchor, and the matched cells MUST be styled with the `viewer.match` role. Each search step MUST be bounded to a fixed window and MUST NOT load the whole file into memory.

#### Scenario: Match straddling a chunk boundary is found

- **WHEN** a search pattern spans the boundary between two consecutive chunks
- **THEN** the overlap of `pattern_len - 1` bytes ensures the match is detected and its offset reported

#### Scenario: Match becomes the top anchor and is highlighted

- **WHEN** F7 finds the next match at a byte offset
- **THEN** that offset becomes the new top-of-screen anchor
- **AND** the matched cells render with the `viewer.match` style

#### Scenario: Search is bounded and streaming

- **WHEN** searching a multi-GB file
- **THEN** each search step reads only a bounded chunk window rather than loading the entire file

### Requirement: Frame-less full-screen chrome and viewer F-key bar

The viewer SHALL render as a frame-less full-screen view replacing the panels, with a header row styled `viewer.header` and body styled `viewer.text`. It MUST display the viewer F-key bar `1Help 2Unwrap 4Hex 7Search 10Quit` using the `keybar.number` and `keybar.label` roles. While open, the viewer SHALL own input focus so its navigation and F2/F4/F7/F10 keys are handled by the viewer rather than the panels, and F10 SHALL close the viewer.

#### Scenario: Viewer F-key bar contents

- **WHEN** the viewer is open
- **THEN** the bottom F-key bar reads `1Help 2Unwrap 4Hex 7Search 10Quit` with number and label styled per the keybar roles

#### Scenario: Viewer owns focus while open

- **WHEN** the viewer is open and the user presses F2, F4, F7, or a navigation key
- **THEN** the viewer handles the key and the underlying panels do not act on it

#### Scenario: F10 closes the viewer

- **WHEN** the user presses F10 in the viewer
- **THEN** the viewer closes and focus returns to the panels
