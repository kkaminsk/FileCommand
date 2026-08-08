//! Centralized `\\?\` long-path abstraction for `fs_ops`. Every concrete
//! [`crate::fs_ops::fs::FileSystem`] implementation routes its paths through
//! [`to_extended_path`] before touching the real filesystem so no caller
//! hand-builds a prefixed path itself.
//!
//! Normalization is purely lexical (`.`/`..` components resolved in-place):
//! it never touches the filesystem and never follows symlinks/reparse
//! points, since `fs_ops` needs to reason about reparse points itself rather
//! than have them silently resolved away underneath it.

use std::path::{Component, Path, PathBuf};

/// Lexically resolve `.` and `..` components without touching the
/// filesystem. A leading `..` past the root is simply dropped (there is
/// nothing above the root to pop).
pub fn lexically_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    // Nothing to pop (already at root, or relative with no
                    // leading components yet) — drop it rather than
                    // producing a bogus `..` in an otherwise-absolute path.
                    if out.as_os_str().is_empty() {
                        out.push(Component::ParentDir.as_os_str());
                    }
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn already_prefixed(path: &Path) -> bool {
    path.as_os_str().to_string_lossy().starts_with(r"\\?\")
}

/// Apply the `\\?\` (or `\\?\UNC\` for UNC paths) long-path prefix to an
/// absolute, lexically-normalized path so `fs_ops` operations are not
/// subject to `MAX_PATH`. A no-op on non-Windows targets, for relative
/// paths, and for paths that are already prefixed.
#[cfg(windows)]
pub fn to_extended_path(path: &Path) -> PathBuf {
    if already_prefixed(path) {
        return path.to_path_buf();
    }
    let normalized = lexically_normalize(path);
    let s = normalized.as_os_str().to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\") {
        PathBuf::from(format!(r"\\?\UNC\{rest}"))
    } else if normalized.is_absolute() {
        PathBuf::from(format!(r"\\?\{s}"))
    } else {
        normalized
    }
}

#[cfg(not(windows))]
pub fn to_extended_path(path: &Path) -> PathBuf {
    lexically_normalize(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_resolves_dot_and_dotdot() {
        assert_eq!(lexically_normalize(Path::new("a/./b/../c")), PathBuf::from("a/c"));
    }

    #[test]
    fn normalize_leaves_leading_dotdot_on_relative_path() {
        assert_eq!(lexically_normalize(Path::new("../a")), PathBuf::from("../a"));
    }

    #[cfg(windows)]
    #[test]
    fn extended_path_prefixes_absolute_and_normalizes_first() {
        let p = to_extended_path(Path::new(r"C:\a\.\b\..\c"));
        assert_eq!(p, PathBuf::from(r"\\?\C:\a\c"));
    }

    #[cfg(windows)]
    #[test]
    fn extended_path_handles_unc() {
        let p = to_extended_path(Path::new(r"\\server\share\file"));
        assert_eq!(p, PathBuf::from(r"\\?\UNC\server\share\file"));
    }

    #[cfg(windows)]
    #[test]
    fn extended_path_is_idempotent() {
        let once = to_extended_path(Path::new(r"C:\a\b"));
        let twice = to_extended_path(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn extended_path_leaves_relative_alone() {
        let p = to_extended_path(Path::new("relative/path"));
        assert_eq!(p, PathBuf::from("relative/path"));
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn extended_path_idempotent_for_arbitrary_segments(a in "[a-zA-Z0-9_]{1,8}", b in "[a-zA-Z0-9_]{1,8}") {
                #[cfg(windows)]
                let p = PathBuf::from(format!(r"C:\{a}\{b}"));
                #[cfg(not(windows))]
                let p = PathBuf::from(format!("/{a}/{b}"));
                let once = to_extended_path(&p);
                let twice = to_extended_path(&once);
                prop_assert_eq!(once, twice);
            }

            #[test]
            fn extended_path_unc_always_uses_unc_prefix(host in "[a-zA-Z0-9_]{1,8}", share in "[a-zA-Z0-9_]{1,8}") {
                #[cfg(windows)]
                {
                    let p = PathBuf::from(format!(r"\\{host}\{share}"));
                    let out = to_extended_path(&p);
                    prop_assert!(out.to_string_lossy().starts_with(r"\\?\UNC\"));
                }
                let _ = (host, share);
            }

            #[test]
            fn normalize_removes_all_curdir_components(segments in prop::collection::vec("[a-zA-Z0-9_]{1,6}", 0..6)) {
                let mut raw = PathBuf::new();
                for s in &segments {
                    raw.push(s);
                    raw.push(".");
                }
                let normalized = lexically_normalize(&raw);
                prop_assert!(!normalized.components().any(|c| c == Component::CurDir));
            }
        }
    }
}
