//! Crash-safe whole-file replacement (editor-encoding-safety; design D4).
//!
//! [`replace_file`] writes new content to a uniquely named temporary file in
//! the target's directory, flushes it to stable storage, and only then swaps
//! it over the original in one rename-style step. At every moment the file
//! at the target path holds either its complete previous content or its
//! complete new content — never a truncated mix — and a failure at any step
//! leaves the original byte-for-byte intact with the temporary file removed.
//!
//! On Windows the swap is `ReplaceFileW`, which carries the original's
//! creation time, attributes, DACL and alternate data streams (for example
//! `Zone.Identifier`) over to the replacement; elsewhere it is
//! `std::fs::rename` followed by a best-effort directory `sync_all`.
//!
//! The helper is deliberately generic — a path and the bytes — so the F4
//! editor's F2 save, `config.toml` rewrites and `history.json` persistence
//! can all share it instead of each growing its own temp-file-and-rename
//! code. Every path goes through [`to_extended_path`] (design D9).

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::fs_ops::path::to_extended_path;

/// Suffix of the temporary file the new content is written to before it
/// replaces the original. Distinctive and dotfile-styled so a leftover from
/// a crash mid-save is recognisable and inert.
const TMP_SUFFIX: &str = "fc-tmp";
/// Suffix of the backup name handed to `ReplaceFileW` on Windows; deleted on
/// success, and only ever left behind by a crash inside the replace itself.
#[cfg(windows)]
const BAK_SUFFIX: &str = "fc-bak";
/// How many sibling names to try before giving up on a unique temporary
/// file (a collision means another save of the same file by the same
/// process is already in flight, which `create_new` makes visible).
const MAX_NAME_ATTEMPTS: u32 = 64;

/// Atomically replace the file at `path` with `bytes`, keeping its metadata.
///
/// Steps (design D4): resolve a symbolic link or other name-surrogate
/// reparse point to its final target (the link itself stays a link);
/// snapshot the original's metadata; create a `.<name>.<pid>.<n>.fc-tmp`
/// sibling with `create_new`, carrying the original's hidden/system/
/// archive/not-content-indexed attributes (Windows) or permission mode
/// (elsewhere); write everything and `sync_all`; then replace the original.
/// Any failure before the replace succeeds removes the temporary file and
/// returns the error unchanged, with the original untouched. A missing
/// original (deleted while it was being edited) is simply created.
pub fn replace_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = resolve_target(&to_extended_path(path))?;
    let original = match std::fs::metadata(&target) {
        Ok(meta) => Some(meta),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let (temp, file) = create_temp(&target, original.as_ref())?;
    let result = write_and_replace(&target, &temp, file, bytes, original.is_some());
    if result.is_err() {
        // Cleanup on failure: the original is untouched and the partial or
        // complete-but-unswapped temporary file must not be left behind.
        // `ReplaceFileW` partial-failure recovery (Windows) has already put
        // the original back under its own name by the time we get here.
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Follow a symbolic link (or, on Windows, a junction or other name-
/// surrogate reparse point) to the file it names, so the *target* is
/// replaced and the link stays a link — mirroring what `std::fs::write`
/// through the link did. A plain file, or a path that does not exist yet,
/// is returned as-is.
fn resolve_target(path: &Path) -> io::Result<PathBuf> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => std::fs::canonicalize(path),
        Ok(_) => Ok(path.to_path_buf()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(e) => Err(e),
    }
}

/// A `.<name>.<pid>.<n>.<suffix>` sibling of `target`.
fn sibling_name(target: &Path, n: u32, suffix: &str) -> io::Result<PathBuf> {
    let parent = target.parent().filter(|p| !p.as_os_str().is_empty()).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target path has no parent directory"))?;
    let name = target.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target path has no file name"))?;
    let mut tmp = std::ffi::OsString::from(".");
    tmp.push(name);
    tmp.push(format!(".{}.{n}.{suffix}", std::process::id()));
    Ok(parent.join(tmp))
}

/// Create the temporary file next to `target` with `create_new`, retrying
/// with the next `<n>` on a name collision. The original's attribute bits
/// (Windows) or permission mode (elsewhere) are applied at creation so a
/// plain `rename` swap would already preserve them; on Windows
/// `ReplaceFileW` makes this redundant but harmless (design D4).
fn create_temp(target: &Path, original: Option<&std::fs::Metadata>) -> io::Result<(PathBuf, File)> {
    for n in 0..MAX_NAME_ATTEMPTS {
        let temp = sibling_name(target, n, TMP_SUFFIX)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        apply_original_metadata(&mut options, original);
        match options.open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "could not find a free temporary file name next to the target"))
}

#[cfg(windows)]
fn apply_original_metadata(options: &mut OpenOptions, original: Option<&std::fs::Metadata>) {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NOT_CONTENT_INDEXED, FILE_ATTRIBUTE_SYSTEM};
    const CARRIED: u32 = FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM | FILE_ATTRIBUTE_ARCHIVE | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED;
    if let Some(meta) = original {
        let bits = meta.file_attributes() & CARRIED;
        if bits != 0 {
            options.attributes(bits);
        }
    }
}

#[cfg(unix)]
fn apply_original_metadata(options: &mut OpenOptions, original: Option<&std::fs::Metadata>) {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    if let Some(meta) = original {
        options.mode(meta.permissions().mode());
    }
}

#[cfg(not(any(windows, unix)))]
fn apply_original_metadata(_options: &mut OpenOptions, _original: Option<&std::fs::Metadata>) {}

/// Write, flush, close, then swap. Split out from [`replace_file`] so the
/// caller has one place to remove the temporary file on any error.
fn write_and_replace(target: &Path, temp: &Path, mut file: File, bytes: &[u8], original_exists: bool) -> io::Result<()> {
    write_all(&mut file, bytes)?;
    file.sync_all()?;
    drop(file);
    before_replace()?;
    replace(target, temp, original_exists)
}

/// `File::write_all`, with the test-only mid-write fault seam in front of
/// it (task 2.5) so "disk full after N bytes" is unit-testable.
fn write_all(file: &mut File, bytes: &[u8]) -> io::Result<()> {
    #[cfg(test)]
    if let Some(n) = fault::take_after_bytes() {
        file.write_all(&bytes[..n.min(bytes.len())])?;
        return Err(io::Error::new(io::ErrorKind::StorageFull, "injected write failure"));
    }
    file.write_all(bytes)
}

/// The test-only "fail just before the replace" fault seam (task 2.5): the
/// temporary file is complete and flushed, the original still untouched.
fn before_replace() -> io::Result<()> {
    #[cfg(test)]
    if fault::take_before_replace() {
        return Err(io::Error::other("injected pre-replace failure"));
    }
    Ok(())
}

/// Windows swap: `ReplaceFileW` with a sibling backup name and the two
/// "ignore" flags, so volumes without ACLs or named streams are non-fatal.
/// The documented partial-failure codes are recovered with both copies
/// intact, and a vanished original falls back to a plain rename.
#[cfg(windows)]
fn replace(target: &Path, temp: &Path, original_exists: bool) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_UNABLE_TO_MOVE_REPLACEMENT, ERROR_UNABLE_TO_MOVE_REPLACEMENT_2, ERROR_UNABLE_TO_REMOVE_REPLACED};
    use windows_sys::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_IGNORE_ACL_ERRORS, REPLACEFILE_IGNORE_MERGE_ERRORS};

    if !original_exists {
        // Deleted while it was being edited: nothing to carry metadata
        // over from, so a plain rename creates it.
        return std::fs::rename(temp, target);
    }

    let backup = sibling_name(target, 0, BAK_SUFFIX)?;
    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain(std::iter::once(0)).collect() };
    let target_w = wide(target);
    let temp_w = wide(temp);
    let backup_w = wide(&backup);

    // SAFETY: the three path arguments are NUL-terminated UTF-16 buffers
    // that live for the duration of the call; the exclude and reserved
    // pointers are documented as reserved and passed as null. The call
    // reads the buffers only.
    let ok = unsafe {
        ReplaceFileW(target_w.as_ptr(), temp_w.as_ptr(), backup_w.as_ptr(), REPLACEFILE_IGNORE_MERGE_ERRORS | REPLACEFILE_IGNORE_ACL_ERRORS, std::ptr::null(), std::ptr::null())
    };
    if ok != 0 {
        // The backup is only a safety net for the swap itself; failing to
        // delete it is not a save failure.
        let _ = std::fs::remove_file(&backup);
        return Ok(());
    }

    let err = io::Error::last_os_error();
    match err.raw_os_error().map(|c| c as u32) {
        // The original vanished between the metadata snapshot and the
        // swap: create it from the temporary file instead.
        Some(ERROR_FILE_NOT_FOUND) if !target.exists() => std::fs::rename(temp, target),
        // Both files keep their own names; the caller removes the temp.
        Some(ERROR_UNABLE_TO_REMOVE_REPLACED) | Some(ERROR_UNABLE_TO_MOVE_REPLACEMENT) => Err(err),
        // The original was moved to the backup name but the replacement
        // could not take its place: put the original back first.
        Some(ERROR_UNABLE_TO_MOVE_REPLACEMENT_2) => {
            let _ = std::fs::rename(&backup, target);
            Err(err)
        }
        _ => Err(err),
    }
}

/// Non-Windows swap: `rename` is atomic on POSIX filesystems; the parent
/// directory `sync_all` makes the new directory entry durable too, but is
/// best-effort because some filesystems refuse to fsync a directory.
#[cfg(not(windows))]
fn replace(target: &Path, temp: &Path, _original_exists: bool) -> io::Result<()> {
    std::fs::rename(temp, target)?;
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

/// Test-only fault injection (task 2.5). A fault is armed per thread and
/// consumed by the one `replace_file` call that reaches it, so parallel
/// tests never see each other's faults.
#[cfg(test)]
pub(crate) mod fault {
    use std::cell::Cell;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Fault {
        /// The write stops after this many bytes with `StorageFull`.
        AfterBytes(usize),
        /// Everything is written and flushed, then the step just before
        /// the replace fails.
        BeforeReplace,
    }

    thread_local! {
        static ARMED: Cell<Option<Fault>> = const { Cell::new(None) };
    }

    pub fn arm(fault: Fault) {
        ARMED.with(|c| c.set(Some(fault)));
    }

    pub(super) fn take_after_bytes() -> Option<usize> {
        ARMED.with(|c| match c.get() {
            Some(Fault::AfterBytes(n)) => {
                c.set(None);
                Some(n)
            }
            _ => None,
        })
    }

    pub(super) fn take_before_replace() -> bool {
        ARMED.with(|c| match c.get() {
            Some(Fault::BeforeReplace) => {
                c.set(None);
                true
            }
            _ => false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::fault::{arm, Fault};
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("filecommand-durable-test-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Names in `dir` other than the target itself — any `.fc-tmp` or
    /// `.fc-bak` leftovers show up here.
    fn leftovers(dir: &Path, target: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| Some(n.as_str()) != target.file_name().and_then(|f| f.to_str())).collect();
        names.sort();
        names
    }

    #[test]
    fn success_replaces_the_content_and_leaves_no_temp_or_backup_files() {
        let dir = temp_dir("success");
        let target = dir.join("notes.txt");
        std::fs::write(&target, b"old content\n").unwrap();
        replace_file(&target, b"new content\n").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new content\n");
        assert_eq!(leftovers(&dir, &target), Vec::<String>::new());
    }

    #[test]
    fn failure_mid_write_leaves_the_original_intact_and_no_temp_sibling() {
        let dir = temp_dir("mid-write");
        let target = dir.join("notes.txt");
        std::fs::write(&target, b"old content\n").unwrap();
        arm(Fault::AfterBytes(3));
        let err = replace_file(&target, b"new content that is longer\n").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::StorageFull);
        assert_eq!(std::fs::read(&target).unwrap(), b"old content\n", "the original is byte-identical");
        assert_eq!(leftovers(&dir, &target), Vec::<String>::new(), "no .fc-tmp sibling remains");
    }

    #[test]
    fn failure_before_the_replace_leaves_the_original_intact() {
        let dir = temp_dir("pre-replace");
        let target = dir.join("notes.txt");
        std::fs::write(&target, b"old content\n").unwrap();
        arm(Fault::BeforeReplace);
        replace_file(&target, b"new content\n").unwrap_err();
        assert_eq!(std::fs::read(&target).unwrap(), b"old content\n");
        assert_eq!(leftovers(&dir, &target), Vec::<String>::new());
    }

    #[test]
    fn a_missing_original_is_created() {
        let dir = temp_dir("missing");
        let target = dir.join("fresh.txt");
        assert!(!target.exists());
        replace_file(&target, b"brand new\n").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"brand new\n");
        assert_eq!(leftovers(&dir, &target), Vec::<String>::new());
    }

    #[test]
    fn a_target_without_a_parent_directory_is_an_error_not_a_panic() {
        let dir = temp_dir("no-parent");
        let target = dir.join("nope").join("deeper.txt");
        let err = replace_file(&target, b"x").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn the_temporary_file_takes_a_fresh_name_when_a_sibling_already_exists() {
        let dir = temp_dir("collision");
        let target = dir.join("notes.txt");
        std::fs::write(&target, b"old\n").unwrap();
        let taken = sibling_name(&to_extended_path(&target), 0, TMP_SUFFIX).unwrap();
        std::fs::write(&taken, b"someone else's save in flight").unwrap();
        replace_file(&target, b"new\n").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new\n");
        assert_eq!(std::fs::read(&taken).unwrap(), b"someone else's save in flight", "the pre-existing sibling is never touched");
        let _ = std::fs::remove_file(&taken);
    }

    #[cfg(windows)]
    #[test]
    fn hidden_and_system_attributes_and_creation_time_survive_on_windows() {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM};
        let dir = temp_dir("attrs");
        let target = dir.join("hidden.txt");
        std::fs::write(&target, b"old\n").unwrap();
        let status = std::process::Command::new("attrib").arg("+H").arg("+S").arg(&target).status().unwrap();
        assert!(status.success(), "attrib +H +S");
        let before = std::fs::metadata(&target).unwrap();
        assert_ne!(before.file_attributes() & FILE_ATTRIBUTE_HIDDEN, 0);
        let created = before.created().unwrap();

        replace_file(&target, b"new\n").unwrap();

        let after = std::fs::metadata(&target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new\n");
        assert_ne!(after.file_attributes() & FILE_ATTRIBUTE_HIDDEN, 0, "hidden survives");
        assert_ne!(after.file_attributes() & FILE_ATTRIBUTE_SYSTEM, 0, "system survives");
        assert_eq!(after.created().unwrap(), created, "creation time is unchanged");
        assert_eq!(leftovers(&dir, &target), Vec::<String>::new());
        let _ = std::process::Command::new("attrib").arg("-H").arg("-S").arg(&target).status();
    }

    #[cfg(windows)]
    #[test]
    fn an_alternate_data_stream_survives_on_windows() {
        let dir = temp_dir("ads");
        let target = dir.join("download.ps1");
        std::fs::write(&target, b"Write-Host hi\n").unwrap();
        let stream = format!("{}:Zone.Identifier", target.display());
        std::fs::write(&stream, b"[ZoneTransfer]\r\nZoneId=3\r\n").unwrap();

        replace_file(&target, b"Write-Host bye\n").unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"Write-Host bye\n");
        assert_eq!(std::fs::read(&stream).unwrap(), b"[ZoneTransfer]\r\nZoneId=3\r\n", "Zone.Identifier survives with the same content");
    }

    #[cfg(unix)]
    #[test]
    fn saving_through_a_symlink_replaces_the_target_and_keeps_the_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("symlink");
        let real = dir.join("real.txt");
        std::fs::write(&real, b"old\n").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
        let link = dir.join("link.txt");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        replace_file(&link, b"new\n").unwrap();

        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link stays a link");
        assert_eq!(std::fs::read(&real).unwrap(), b"new\n", "the link target holds the new content");
        assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o640, "the permission mode is preserved");
    }
}
