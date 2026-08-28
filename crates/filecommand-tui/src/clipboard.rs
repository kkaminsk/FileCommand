//! The OS clipboard write behind `Effect::SetClipboard`
//! (clipboard-export). `core::update` only ever produces a
//! `ClipboardPayload` — which of the three payload kinds, and the resolved,
//! `..`-excluded absolute paths to act on; it never touches the OS. This
//! module is where that touches real Win32 (`clipboard-win`) or, off
//! Windows, a best-effort text-only clipboard (`arboard`), plus the path
//! normalisation both need (design D3, D4, D5).
//!
//! [`Clipboard`] is a trait, not a concrete type, purely so `app.rs`'s
//! `run_effects` can be exercised in tests against [`RecordingClipboard`]
//! without a real OS clipboard (which CI has no window station to open on
//! Windows, and doesn't exist at all on the non-Windows runners this crate
//! also has to build on).

use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// Executes one clipboard-export payload write. `run_effects` calls this
/// synchronously on the UI thread for `Effect::SetClipboard`, exactly like
/// `EnumerateDrives` (design D3) — clipboard open/set/close is sub-
/// millisecond once the clipboard isn't contested, and the busy case is
/// bounded by each implementation's own retry (clipboard-export "Clipboard
/// busy retry").
pub trait Clipboard {
    /// Write `items` (already [`normalize_path`]-d, absolute paths) to the
    /// clipboard as file objects — `CF_HDROP` plus `Preferred DropEffect =
    /// COPY` on Windows (clipboard-export "Windows file-object payload").
    ///
    /// `Ok(true)` means the platform has no file-object support and `items`
    /// were written as the Paths text payload instead — the caller reports
    /// that as `fell_back_to_paths` (clipboard-export "Non-Windows
    /// fallback"). `Ok(false)` means real file objects were written.
    /// `Err(message)` is a user-facing failure, already retried where that
    /// makes sense (clipboard-export "Clipboard busy retry").
    fn set_files(&self, items: &[PathBuf]) -> Result<bool, String>;

    /// Write `text` as plain Unicode text (the Paths/Names payloads, and
    /// Files' non-Windows fallback).
    fn set_text(&self, text: &str) -> Result<(), String>;
}

/// Normalise one clipboard item's path (clipboard-export "Windows
/// file-object payload"; design D4). Items already arrive absolute —
/// `core::update` builds them as `cwd.join(name)` — so the only surgery
/// needed here is stripping a `\\?\` long-path prefix and rewriting
/// `\\?\UNC\server\share\...` to `\\server\share\...`; Explorer's
/// `DragQueryFileW` rejects the verbatim-path prefix on paste, and a user
/// pasting the Paths text into an editor or chat has no use for it either.
/// A no-op on any path that never had the prefix, which is every path on a
/// non-Windows host, so this needs no `cfg` gate and is plain-value testable
/// everywhere.
pub fn normalize_path(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path.to_path_buf()
}

/// The Paths payload's text: one absolute path per line, no trailing
/// separator after the last (clipboard-export "Clipboard payloads and
/// scope").
pub fn paths_text(items: &[PathBuf]) -> String {
    items.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n")
}

/// The Names payload's text: one file name per line, no trailing separator
/// after the last (clipboard-export "Clipboard payloads and scope").
pub fn names_text(items: &[PathBuf]) -> String {
    items
        .iter()
        .map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(windows)]
mod windows_impl {
    use std::path::PathBuf;
    use std::time::Duration;

    use clipboard_win::{formats, raw, Clipboard as WinHandle, Setter};

    use super::Clipboard;

    /// `clipboard-win`'s own `Clipboard::new_attempts` only yields the
    /// scheduler between tries (`Sleep(0)`), not a real delay, so it alone
    /// doesn't give another process holding the clipboard a chance to
    /// release it. This wrapper retries `Clipboard::new()` itself with an
    /// actual back-off (clipboard-export "Clipboard busy retry": "a short
    /// back-off"; design D3's "`Clipboard::new_attempts(5)` with ~20 ms
    /// back-off" describes the attempt count and interval this reproduces).
    const OPEN_ATTEMPTS: usize = 5;
    const OPEN_BACKOFF: Duration = Duration::from_millis(20);
    const BUSY_MESSAGE: &str = "Clipboard busy — try again";

    /// `DROPEFFECT_COPY` (`oleidl.h`): the 4-byte little-endian value
    /// Explorer reads from `Preferred DropEffect` to decide a paste copies
    /// rather than moves the dropped files (clipboard-export "Windows
    /// file-object payload").
    const DROPEFFECT_COPY: u32 = 1;
    const PREFERRED_DROP_EFFECT: &str = "Preferred DropEffect";

    /// The real Windows clipboard, backed by `clipboard-win` 5.x (design
    /// D3).
    #[derive(Debug, Default)]
    pub struct WindowsClipboard;

    impl WindowsClipboard {
        pub fn new() -> Self {
            Self
        }
    }

    /// Bounded retry with an actual back-off between attempts — see
    /// `OPEN_ATTEMPTS`/`OPEN_BACKOFF` above. At most `OPEN_ATTEMPTS - 1`
    /// sleeps of `OPEN_BACKOFF` each (well under a second total), never
    /// blocking the UI thread for more than "a fraction of a second"
    /// (clipboard-export "Clipboard busy retry").
    fn open_with_retry() -> Result<WinHandle, String> {
        for attempt in 0..OPEN_ATTEMPTS {
            match WinHandle::new() {
                Ok(clip) => return Ok(clip),
                Err(_) if attempt + 1 < OPEN_ATTEMPTS => std::thread::sleep(OPEN_BACKOFF),
                Err(_) => return Err(BUSY_MESSAGE.to_string()),
            }
        }
        Err(BUSY_MESSAGE.to_string())
    }

    impl Clipboard for WindowsClipboard {
        fn set_files(&self, items: &[PathBuf]) -> Result<bool, String> {
            let _clip = open_with_retry()?;
            // `FileList`'s own setter (`raw::set_file_list`) does not clear
            // the clipboard first (it exists to be layered under a second
            // format, exactly as here), so this call owns the one clear for
            // both formats written in this open/close cycle.
            raw::empty().map_err(|_| BUSY_MESSAGE.to_string())?;
            let paths: Vec<String> = items.iter().map(|p| p.display().to_string()).collect();
            formats::FileList.write_clipboard(&paths).map_err(|e| e.to_string())?;
            // Best-effort: a paste still copies the files without the
            // preferred-effect hint (it just falls back to the OS/app
            // default), so a failure here doesn't fail the whole action.
            if let Some(cf) = raw::register_format(PREFERRED_DROP_EFFECT) {
                let bytes = DROPEFFECT_COPY.to_le_bytes();
                let _ = raw::set_without_clear(cf.get(), &bytes);
            }
            Ok(false)
        }

        fn set_text(&self, text: &str) -> Result<(), String> {
            let _clip = open_with_retry()?;
            formats::Unicode.write_clipboard(&text).map_err(|e| e.to_string())
        }
    }
}

#[cfg(windows)]
pub use windows_impl::WindowsClipboard as PlatformClipboard;

#[cfg(not(windows))]
mod text_impl {
    use std::path::PathBuf;

    use arboard::Clipboard as ArboardHandle;

    use super::{paths_text, Clipboard};

    /// Best-effort non-Windows clipboard: `arboard` text only. Files falls
    /// back to the Paths text payload (clipboard-export "Non-Windows
    /// fallback"); `text/uri-list` target negotiation is out of scope
    /// (design D5).
    #[derive(Debug, Default)]
    pub struct TextClipboard;

    impl TextClipboard {
        pub fn new() -> Self {
            Self
        }
    }

    impl Clipboard for TextClipboard {
        fn set_files(&self, items: &[PathBuf]) -> Result<bool, String> {
            self.set_text(&paths_text(items))?;
            Ok(true)
        }

        fn set_text(&self, text: &str) -> Result<(), String> {
            let mut clip = ArboardHandle::new().map_err(|e| e.to_string())?;
            clip.set_text(text.to_string()).map_err(|e| e.to_string())
        }
    }
}

#[cfg(not(windows))]
pub use text_impl::TextClipboard as PlatformClipboard;

/// A fake [`Clipboard`] that records every call instead of touching the OS
/// clipboard, so TUI tests can exercise `app::run_effects`'s `SetClipboard`
/// arm deterministically on any host, including CI, where a real Windows
/// clipboard may not be openable (no window station) and a real non-Windows
/// clipboard may not exist at all.
#[derive(Debug, Default)]
pub struct RecordingClipboard {
    pub files_calls: RefCell<Vec<Vec<PathBuf>>>,
    pub text_calls: RefCell<Vec<String>>,
    /// When `Some`, both methods return this `Err` instead of recording a
    /// call — exercises the busy/failure path (clipboard-export "Clipboard
    /// busy retry": "Persistent lock reports failure").
    pub fail_with: Option<String>,
    /// When `true`, `set_files` behaves like the non-Windows fallback:
    /// records the call (as a `set_files` call, not `set_text` — callers
    /// that need the fallen-back-to-text distinction check the return
    /// value) and reports `Ok(true)`.
    pub simulate_fallback: bool,
}

impl Clipboard for RecordingClipboard {
    fn set_files(&self, items: &[PathBuf]) -> Result<bool, String> {
        if let Some(message) = &self.fail_with {
            return Err(message.clone());
        }
        self.files_calls.borrow_mut().push(items.to_vec());
        Ok(self.simulate_fallback)
    }

    fn set_text(&self, text: &str) -> Result<(), String> {
        if let Some(message) = &self.fail_with {
            return Err(message.clone());
        }
        self.text_calls.borrow_mut().push(text.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_path_leaves_an_ordinary_absolute_path_unchanged() {
        let p = PathBuf::from(r"C:\NORTON\README.md");
        assert_eq!(normalize_path(&p), p);
    }

    /// clipboard-export "Windows file-object payload": "Long-path prefix is
    /// stripped".
    #[test]
    fn normalize_path_strips_the_long_path_prefix() {
        let p = PathBuf::from(r"\\?\C:\very\long\path\file.txt");
        assert_eq!(normalize_path(&p), PathBuf::from(r"C:\very\long\path\file.txt"));
    }

    /// clipboard-export "Windows file-object payload": "UNC prefix is
    /// rewritten".
    #[test]
    fn normalize_path_rewrites_the_unc_prefix() {
        let p = PathBuf::from(r"\\?\UNC\srv\share\dir\file.txt");
        assert_eq!(normalize_path(&p), PathBuf::from(r"\\srv\share\dir\file.txt"));
    }

    #[test]
    fn paths_text_joins_with_no_trailing_separator() {
        let items = vec![PathBuf::from(r"C:\a.txt"), PathBuf::from(r"C:\b.txt")];
        assert_eq!(paths_text(&items), "C:\\a.txt\nC:\\b.txt");
    }

    #[test]
    fn paths_text_of_a_single_item_has_no_newline() {
        let items = vec![PathBuf::from(r"C:\a.txt")];
        assert_eq!(paths_text(&items), "C:\\a.txt");
    }

    #[test]
    fn names_text_extracts_the_file_name_component() {
        let items = vec![PathBuf::from(r"C:\NORTON\a.txt"), PathBuf::from(r"C:\NORTON\sub\b.txt")];
        assert_eq!(names_text(&items), "a.txt\nb.txt");
    }

    #[test]
    fn recording_clipboard_records_set_files_calls() {
        let clip = RecordingClipboard::default();
        let items = vec![PathBuf::from(r"C:\a.txt")];
        assert_eq!(clip.set_files(&items), Ok(false));
        assert_eq!(clip.files_calls.borrow().as_slice(), &[items]);
    }

    #[test]
    fn recording_clipboard_can_simulate_the_non_windows_fallback() {
        let clip = RecordingClipboard { simulate_fallback: true, ..Default::default() };
        let items = vec![PathBuf::from(r"C:\a.txt")];
        assert_eq!(clip.set_files(&items), Ok(true));
    }

    #[test]
    fn recording_clipboard_can_simulate_a_persistent_failure() {
        let clip = RecordingClipboard { fail_with: Some("Clipboard busy — try again".to_string()), ..Default::default() };
        assert_eq!(clip.set_text("x"), Err("Clipboard busy — try again".to_string()));
        assert!(clip.text_calls.borrow().is_empty());
    }
}
