//! Windows-only integration test for the real OS clipboard behind
//! `filecommand_tui::clipboard::PlatformClipboard` (clipboard-export
//! "Windows file-object payload"; design D3/D4). Everything else in this
//! crate exercises the clipboard through `RecordingClipboard` because CI
//! runners typically have no window station to open a real clipboard on
//! (Windows Server Core, headless Linux/macOS runners); this file is the one
//! place that actually opens the live clipboard, writes to it, and reads it
//! back, so it is `#[ignore]`d and meant to be run explicitly on a real
//! Windows desktop session:
//!
//! ```text
//! cargo test -p filecommand-tui --test clipboard_windows -- --ignored
//! ```
//!
//! The whole file is gated `#![cfg(windows)]` — `clipboard-win` (this test's
//! only way to read back what got written) is itself a
//! `[target.'cfg(windows)'.dependencies]` dependency and doesn't exist on
//! other targets — so the crate still builds cleanly everywhere else
//! (`cargo build --workspace` / `cargo test --workspace`, which does not run
//! `#[ignore]`d tests by default) with this file simply contributing zero
//! test functions off Windows.

#![cfg(windows)]

use std::path::PathBuf;

use clipboard_win::{formats, raw, Clipboard as WinHandle, Getter};

use filecommand_tui::clipboard::{self, Clipboard, PlatformClipboard};

/// `DROPEFFECT_COPY` (`oleidl.h`) — mirrors the private constant in
/// `filecommand_tui::clipboard`'s Windows implementation; duplicated here
/// because that module doesn't expose it (clipboard-export "Windows
/// file-object payload").
const DROPEFFECT_COPY: u32 = 1;
const PREFERRED_DROP_EFFECT: &str = "Preferred DropEffect";

/// Writes a `FileList` (+ `Preferred DropEffect`) through
/// `PlatformClipboard::set_files` exactly as `app::run_effects` does for
/// `Effect::SetClipboard` — `crate::clipboard::normalize_path` applied to
/// each item first — then reopens the clipboard independently (via the raw
/// `clipboard-win` API, not our own trait) to read every part of it back.
#[test]
#[ignore = "opens the real Windows clipboard; needs a desktop window station, so it is not run by `cargo test --workspace`"]
fn windows_clipboard_round_trips_a_real_file_list() {
    let temp_dir = std::env::temp_dir().join("filecommand-clipboard-roundtrip-test");
    std::fs::create_dir_all(&temp_dir).expect("create temp dir for the round-trip fixture");
    let real_file = temp_dir.join("report.docx");
    std::fs::write(&real_file, b"fixture content").expect("write fixture file");

    // One item arrives internally with the `\\?\` long-path prefix (as
    // `PanelState::cwd` can when Windows returns a verbatim path), one with
    // the `\\?\UNC\` prefix — both must come off before the item reaches the
    // clipboard (clipboard-export "Windows file-object payload": "Long-path
    // prefix is stripped", "UNC prefix is rewritten").
    let long_path_item = PathBuf::from(format!(r"\\?\{}", real_file.display()));
    let unc_item = PathBuf::from(r"\\?\UNC\srv\share\dir\file.txt");

    let raw_items = vec![real_file.clone(), long_path_item, unc_item];
    let normalized: Vec<PathBuf> = raw_items.iter().map(|p| clipboard::normalize_path(p)).collect();

    assert_eq!(normalized[0], real_file, "a plain absolute path is left unchanged");
    assert_eq!(normalized[1], real_file, "the `\\\\?\\` long-path prefix must be stripped");
    assert_eq!(normalized[2], PathBuf::from(r"\\srv\share\dir\file.txt"), "`\\\\?\\UNC\\server\\share` must be rewritten to `\\\\server\\share`");
    assert!(!normalized.iter().any(|p| p.display().to_string().contains(r"\\?\")), "no normalized item may still carry the verbatim-path prefix");

    let clip = PlatformClipboard::default();
    let fell_back_to_paths = clip.set_files(&normalized).expect("set_files should succeed against a free clipboard");
    assert!(!fell_back_to_paths, "on Windows, Files must write real file objects, not fall back to text");

    // Read back independently of our own `Clipboard` trait, through
    // `clipboard-win`'s own `Getter`, to confirm what actually landed on the
    // OS clipboard rather than merely what our code believes it wrote.
    // `clipboard-win`'s `Vec<PathBuf>` getter needs its optional `std`
    // feature, which this workspace doesn't enable; `Vec<String>` is the
    // always-available getter, so paths are compared as strings.
    let read_back: Vec<String> = {
        let _handle = WinHandle::new_attempts(10).expect("reopen the clipboard to read back what was just written");
        let mut out: Vec<String> = Vec::new();
        Getter::<Vec<String>>::read_clipboard(&formats::FileList, &mut out).expect("read back the CF_HDROP file list");
        out
    };
    let expected: Vec<String> = normalized.iter().map(|p| p.display().to_string()).collect();
    assert_eq!(read_back, expected, "the file list read back must exactly match the normalized items written");

    // `Preferred DropEffect`: a registered clipboard format carrying the
    // 4-byte little-endian `DROPEFFECT_COPY` (clipboard-export "Windows
    // file-object payload").
    let drop_effect_bytes: Vec<u8> = {
        let _handle = WinHandle::new_attempts(10).expect("reopen the clipboard to read back the drop-effect hint");
        let format = raw::register_format(PREFERRED_DROP_EFFECT).expect("register the well-known `Preferred DropEffect` format name");
        let mut out = Vec::new();
        raw::get_vec(format.get(), &mut out).expect("read back the `Preferred DropEffect` payload");
        out
    };
    assert_eq!(drop_effect_bytes.len(), 4, "`Preferred DropEffect` is a 4-byte DWORD");
    let drop_effect = u32::from_le_bytes(drop_effect_bytes.try_into().unwrap());
    assert_eq!(drop_effect, DROPEFFECT_COPY, "the preferred drop effect must be COPY, never MOVE (no Cut in this change — design Non-Goals)");

    let _ = std::fs::remove_dir_all(&temp_dir);
}
