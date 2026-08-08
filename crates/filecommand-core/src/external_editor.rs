//! Config-driven F4 external-editor dispatch (design D7).
//!
//! This module builds *what* to run (`editor` command + the target file)
//! and *where* (the active panel's directory), mirroring [`crate::shell`]'s
//! split between "construct the invocation" (here, pure) and "spawn it"
//! (`filecommand-tui`, which reuses the same suspend/restore seam `shell`
//! uses for command passthrough). Unlike `shell::Invocation`, the target
//! file argument here is kept as an `OsString` end to end so a filename that
//! is not valid Unicode reaches the editor unmangled (external-editor: F4
//! launches the editor — "Original OS filename passed through").

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::shell;

/// A fully-formed external-editor spawn request: the editor program, the
/// arguments from `editor =` that precede the file, the target file as the
/// final argument (original `OsString`, never lossy-converted), and the
/// spawn working directory (the active panel's current directory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorInvocation {
    pub program: String,
    pub leading_args: Vec<String>,
    pub file_arg: OsString,
    pub cwd: PathBuf,
}

/// Whether `editor =` names a usable command: present and non-blank after
/// trimming. An absent value and an empty/whitespace-only string are both
/// "unset" (external-editor: Config-driven external editor command —
/// "Editor command unset", "Editor selected from config schema key already
/// reserved").
pub fn is_configured(editor: Option<&str>) -> bool {
    editor.map(str::trim).is_some_and(|s| !s.is_empty())
}

/// Why F4 could not be dispatched to the external editor for the panel
/// cursor's current entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetError {
    /// `editor =` is absent or blank.
    Unconfigured,
    /// The cursor is on a directory entry (including `..`).
    IsDirectory,
}

/// Resolve the F4-from-panel target into a spawn request: the configured
/// editor command split into program + leading args (reusing
/// [`shell::split_spec`] so a quoted program path with spaces still parses
/// as one token), the cursor entry's original file name as the final
/// argument, and `cwd` as the spawn working directory.
///
/// `entry_name` / `is_dir` describe the active panel's cursor entry;
/// directories (including `..`) are rejected rather than launched
/// (external-editor: F4 launches the editor — "Launch on file under
/// cursor", "Cursor on a directory entry").
pub fn resolve(editor: Option<&str>, cwd: &Path, entry_name: &OsString, is_dir: bool) -> Result<EditorInvocation, TargetError> {
    let Some(editor) = editor.map(str::trim).filter(|s| !s.is_empty()) else {
        return Err(TargetError::Unconfigured);
    };
    if is_dir {
        return Err(TargetError::IsDirectory);
    }
    let mut parts = shell::split_spec(editor);
    // `is_configured`/the check above guarantee at least one non-blank
    // token, so `split_spec` never returns empty here.
    let program = parts.remove(0);
    Ok(EditorInvocation { program, leading_args: parts, file_arg: entry_name.clone(), cwd: cwd.to_path_buf() })
}

/// The fixed message shown when F4 is pressed with no `editor =` configured
/// (external-editor: Config-driven external editor command — "Editor
/// command unset").
pub const NO_EDITOR_CONFIGURED_MESSAGE: &str = "No editor configured. Set `editor =` in config.toml.";

#[cfg(test)]
mod tests {
    use super::*;

    fn name(s: &str) -> OsString {
        OsString::from(s)
    }

    #[test]
    fn unset_and_blank_editor_are_both_unconfigured() {
        assert!(!is_configured(None));
        assert!(!is_configured(Some("")));
        assert!(!is_configured(Some("   ")));
        assert!(is_configured(Some("notepad")));
    }

    #[test]
    fn resolve_rejects_unconfigured_editor() {
        let err = resolve(None, Path::new(r"C:\work"), &name("report.txt"), false).unwrap_err();
        assert_eq!(err, TargetError::Unconfigured);
        let err = resolve(Some("  "), Path::new(r"C:\work"), &name("report.txt"), false).unwrap_err();
        assert_eq!(err, TargetError::Unconfigured);
    }

    #[test]
    fn resolve_rejects_directory_targets_including_parent() {
        let err = resolve(Some("notepad"), Path::new(r"C:\work"), &name("sub"), true).unwrap_err();
        assert_eq!(err, TargetError::IsDirectory);
        let err = resolve(Some("notepad"), Path::new(r"C:\work"), &name(".."), true).unwrap_err();
        assert_eq!(err, TargetError::IsDirectory);
    }

    #[test]
    fn resolve_launches_on_the_file_under_the_cursor_in_the_panel_directory() {
        let inv = resolve(Some("notepad"), Path::new(r"C:\work"), &name("report.txt"), false).unwrap();
        assert_eq!(inv.program, "notepad");
        assert!(inv.leading_args.is_empty());
        assert_eq!(inv.file_arg, OsString::from("report.txt"));
        assert_eq!(inv.cwd, PathBuf::from(r"C:\work"));
    }

    #[test]
    fn resolve_splits_leading_args_before_the_file_argument() {
        let inv = resolve(Some("code --wait"), Path::new(r"C:\work"), &name("a.rs"), false).unwrap();
        assert_eq!(inv.program, "code");
        assert_eq!(inv.leading_args, vec!["--wait".to_string()]);
        assert_eq!(inv.file_arg, OsString::from("a.rs"));
    }

    #[test]
    fn resolve_handles_a_quoted_program_path_with_spaces() {
        let inv = resolve(Some(r#""C:\Program Files\Editor\ed.exe""#), Path::new(r"C:\work"), &name("a.rs"), false).unwrap();
        assert_eq!(inv.program, r"C:\Program Files\Editor\ed.exe");
        assert!(inv.leading_args.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_passes_the_original_os_string_without_lossy_conversion() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        // Invalid UTF-8 byte sequence: not representable losslessly as a
        // `String`, but perfectly valid as an `OsString` on Unix.
        let raw = OsStr::from_bytes(&[0x66, 0x6f, 0x80, 0x6f]).to_os_string();
        let inv = resolve(Some("notepad"), Path::new("/work"), &raw, false).unwrap();
        assert_eq!(inv.file_arg, raw);
    }

    #[test]
    fn resolve_is_a_pure_computation_needing_no_filesystem_or_process() {
        // The whole point of D7: build the invocation without touching a
        // terminal, spawning anything, or reading a directory.
        let inv = resolve(Some("vim"), Path::new("/tmp"), &name("x"), false).unwrap();
        assert_eq!(inv.program, "vim");
    }
}
