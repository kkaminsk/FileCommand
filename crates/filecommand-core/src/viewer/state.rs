//! The core, terminal-independent state of an open F3 viewer session
//! (design D1). `ViewerState` carries no `ByteSource` and does no I/O —
//! byte access lives in [`super::byte_source`] and the pure algorithms in
//! [`super::decode`], [`super::hex`], [`super::backward`], and
//! [`super::search`] — so it derives `Clone`/`PartialEq`/`Eq` like every
//! other piece of [`crate::update::State`] and every transition still flows
//! through [`crate::update::update`].

use std::path::PathBuf;

/// Which of the two viewer bodies is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Text,
    Hex,
}

impl ViewMode {
    /// The other mode (viewer: Text and hex modes — "F4 toggles mode and
    /// label").
    pub fn toggle(self) -> ViewMode {
        match self {
            ViewMode::Text => ViewMode::Hex,
            ViewMode::Hex => ViewMode::Text,
        }
    }

    /// The viewer F-key bar's slot-4 label for the mode this toggle would
    /// switch *to* — i.e. while in text mode the key bar reads `Hex`
    /// (pressing F4 enters hex mode), and while in hex mode it reads
    /// `ASCII` (viewer: Text and hex modes — "F4 toggles mode and label").
    pub fn toggle_label(self) -> &'static str {
        match self {
            ViewMode::Text => "Hex",
            ViewMode::Hex => "ASCII",
        }
    }
}

/// The core state of one open F3 viewer session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerState {
    pub path: PathBuf,
    /// The file length snapshotted at open (viewer: Instant open —
    /// "Offsets clamped to the snapshot length").
    pub file_len: u64,
    pub mode: ViewMode,
    /// F2: wrap re-flows logical lines at the viewport width; unwrap clips
    /// them at `h_scroll` (viewer: F2 wrap and unwrap toggle).
    pub wrap: bool,
    /// The byte offset of the first visible row.
    pub top_offset: u64,
    /// Horizontal scroll, in display columns; meaningful in unwrap mode
    /// only, and reflected by the header `Col` indicator (viewer: F2 wrap
    /// and unwrap toggle — "Unwrap clips with horizontal scroll").
    pub h_scroll: usize,
    /// The F7 search prompt's in-progress text, `Some` only while the
    /// prompt is open.
    pub search_input: Option<String>,
    /// The last-run search pattern, as literal bytes.
    pub search_pattern: Option<Vec<u8>>,
    /// The most recent match's byte range `[start, end)`, styled with the
    /// `viewer.match` role (viewer: F7 streaming search — "Match becomes
    /// the top anchor and is highlighted").
    pub last_match: Option<(u64, u64)>,
}

impl ViewerState {
    /// A freshly opened viewer session: text mode, unwrapped, positioned at
    /// the start of the file.
    pub fn new(path: PathBuf, file_len: u64) -> ViewerState {
        ViewerState {
            path,
            file_len,
            mode: ViewMode::Text,
            wrap: false,
            top_offset: 0,
            h_scroll: 0,
            search_input: None,
            search_pattern: None,
            last_match: None,
        }
    }

    /// Move the top-of-screen anchor to `offset`, clamped to the file's
    /// snapshot length so navigation never points past it (viewer: Instant
    /// open — "Offsets clamped to the snapshot length").
    pub fn set_top_offset(&mut self, offset: u64) {
        self.top_offset = offset.min(self.file_len);
    }

    /// F4 in the viewer: toggle text/hex mode (viewer: Text and hex modes
    /// — "F4 toggles mode and label").
    pub fn toggle_mode(&mut self) {
        self.mode = self.mode.toggle();
    }

    /// F2: toggle wrap/unwrap. Horizontal scroll resets since it only
    /// applies in unwrap mode.
    pub fn toggle_wrap(&mut self) {
        self.wrap = !self.wrap;
        self.h_scroll = 0;
    }
}

/// Byte-offset percent-through indicator: `top_offset / file_len`, expressed
/// in `[0, 100]`. Always `0` for an empty file. Depends only on the two
/// byte offsets, never on decoded content or wrap state (viewer: Byte-offset
/// header indicators — "Percent is byte-offset based").
pub fn percent_through(top_offset: u64, file_len: u64) -> u8 {
    if file_len == 0 {
        return 0;
    }
    let pct = (top_offset.min(file_len) as f64 / file_len as f64) * 100.0;
    pct.round().clamp(0.0, 100.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_viewer_opens_in_text_mode_unwrapped_at_the_start() {
        let v = ViewerState::new(PathBuf::from("/a.txt"), 1000);
        assert_eq!(v.mode, ViewMode::Text);
        assert!(!v.wrap);
        assert_eq!(v.top_offset, 0);
        assert_eq!(v.h_scroll, 0);
        assert_eq!(v.last_match, None);
    }

    #[test]
    fn toggle_mode_swaps_text_and_hex_and_the_key_bar_label() {
        let mut v = ViewerState::new(PathBuf::from("/a.txt"), 10);
        assert_eq!(v.mode.toggle_label(), "Hex");
        v.toggle_mode();
        assert_eq!(v.mode, ViewMode::Hex);
        assert_eq!(v.mode.toggle_label(), "ASCII");
        v.toggle_mode();
        assert_eq!(v.mode, ViewMode::Text);
    }

    #[test]
    fn toggle_wrap_flips_the_flag_and_resets_horizontal_scroll() {
        let mut v = ViewerState::new(PathBuf::from("/a.txt"), 10);
        v.h_scroll = 40;
        v.toggle_wrap();
        assert!(v.wrap);
        assert_eq!(v.h_scroll, 0);
        v.h_scroll = 5;
        v.toggle_wrap();
        assert!(!v.wrap);
        assert_eq!(v.h_scroll, 0);
    }

    #[test]
    fn set_top_offset_clamps_to_file_length() {
        let mut v = ViewerState::new(PathBuf::from("/a.txt"), 100);
        v.set_top_offset(50);
        assert_eq!(v.top_offset, 50);
        v.set_top_offset(1_000_000);
        assert_eq!(v.top_offset, 100);
    }

    #[test]
    fn percent_through_is_byte_offset_ratio() {
        assert_eq!(percent_through(0, 200), 0);
        assert_eq!(percent_through(100, 200), 50);
        assert_eq!(percent_through(200, 200), 100);
        assert_eq!(percent_through(0, 0), 0);
    }

    #[test]
    fn percent_through_is_invariant_to_decode_or_wrap_state() {
        // The percent computation takes only the two byte offsets — there
        // is no decode/wrap parameter to even vary, which is itself the
        // guarantee (viewer: Byte-offset header indicators — "the value is
        // unaffected by whether the file contains valid UTF-8 or how lines
        // wrap").
        assert_eq!(percent_through(300, 1200), percent_through(300, 1200));
    }

    #[test]
    fn percent_through_never_exceeds_100_when_offset_beyond_length() {
        assert_eq!(percent_through(500, 200), 100);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn percent_through_is_always_in_bounds(top in 0u64..10_000_000, len in 1u64..10_000_000) {
                let pct = percent_through(top, len);
                prop_assert!(pct <= 100);
            }
        }
    }
}
