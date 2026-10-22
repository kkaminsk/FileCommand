//! The narrow filesystem trait seam `fs_ops` operates through. No code
//! anywhere else in `fs_ops` calls `std::fs` directly — every access goes
//! through a [`FileSystem`] implementation, routed through
//! [`crate::fs_ops::path::to_extended_path`].
//!
//! Two implementations exist: [`RealFs`] (the real, Windows-backed
//! filesystem) and, compiled for this crate's unit tests only, `FakeFs` (an
//! in-memory double for deterministic unit tests, including error
//! injection; durable-file-copy design D9).

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::path::to_extended_path;

/// Volume + file-index identity, used for identity-aware existence checks
/// (so a case-only rename is recognized as "the same file") and for
/// reparse-point recursion-cycle protection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId {
    pub volume_serial: u64,
    pub file_index: u64,
}

/// What kind of reparse point a path is, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReparseKind {
    None,
    Symlink,
    Junction,
    Other,
}

impl ReparseKind {
    pub fn is_reparse_point(self) -> bool {
        !matches!(self, ReparseKind::None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsMetadata {
    /// Whether the entry is a directory object. For the non-followed
    /// metadata of a reparse point this means "the link itself is a
    /// directory object" — a Windows junction or `mklink /D` symlink, which
    /// is removed with `remove_dir` — not whether its target is a directory
    /// (move-reparse-point-safety design D4). A Unix symlink is never a
    /// directory object and is removed with `remove_file`. Callers that
    /// recurse check `reparse` first, so a link is never traversed as a
    /// directory by mistake.
    pub is_dir: bool,
    pub reparse: ReparseKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub readonly: bool,
    pub file_id: Option<FileId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEntry {
    pub name: OsString,
    pub metadata: FsMetadata,
}

/// What a copy progress callback tells the caller to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkControl {
    Continue,
    Cancel,
}

/// Options for [`FileSystem::copy_file`] (durable-file-copy design D2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyOptions {
    /// The chunk size for implementations that copy in a read/write loop,
    /// and so the progress-callback cadence. A hint: the Windows
    /// implementation hands the copy to `CopyFileExW`, which chooses its own
    /// chunking.
    pub chunk_bytes: usize,
    /// Flush the destination to stable storage before returning, so a flush
    /// or close failure fails the copy. A move passes `true`, because it is
    /// about to delete the only other copy; a plain copy passes `false`
    /// (design D4).
    pub flush: bool,
}

/// One piece of metadata a copy could not carry over. The `Display` form is
/// the user-facing clause the end-of-job summary shows, followed by the
/// operating system's message (operation-dialogs "Metadata-restore failures
/// are reported in the end-of-job summary").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnrestoredKind {
    /// A named alternate data stream that is missing on the copy.
    Stream(OsString),
    /// The source's (or the copy's) stream list could not be read, so
    /// nothing is known about its streams.
    StreamListing,
    ModifiedTime,
    CreatedTime,
    ReadOnly,
}

impl std::fmt::Display for UnrestoredKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnrestoredKind::Stream(name) => write!(f, "stream {} not copied", name.to_string_lossy()),
            UnrestoredKind::StreamListing => f.write_str("alternate data streams not listed"),
            UnrestoredKind::ModifiedTime => f.write_str("modification time not restored"),
            UnrestoredKind::CreatedTime => f.write_str("creation time not restored"),
            UnrestoredKind::ReadOnly => f.write_str("read-only attribute not restored"),
        }
    }
}

/// Metadata a [`FileSystem::copy_file`] could not carry over, with the
/// operating system's message. Never fails the copy and never swallowed
/// inside the filesystem layer: the worker turns each one into a summary
/// warning (durable-file-copy design D2; error-diagnostics design D5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unrestored {
    pub what: UnrestoredKind,
    pub error: String,
}

impl Unrestored {
    pub fn new(what: UnrestoredKind, error: impl ToString) -> Unrestored {
        Unrestored { what, error: error.to_string() }
    }

    /// `"<what>: <os message>"`, the row the end-of-job summary shows.
    pub fn reason(&self) -> String {
        format!("{}: {}", self.what, self.error)
    }
}

/// The outcome of a successful [`FileSystem::copy_file`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CopyReport {
    /// Bytes of the main data stream — the same quantity the progress
    /// callback reports, and the length the destination was checked against.
    pub bytes: u64,
    pub unrestored: Vec<Unrestored>,
}

/// The narrow fs-access seam. `metadata` never follows a reparse point
/// (lstat-like); `metadata_follow` always does (stat-like) — `fs_ops` needs
/// both to implement reparse-point semantics correctly.
pub trait FileSystem: Send + Sync {
    fn metadata(&self, path: &Path) -> io::Result<FsMetadata>;
    fn metadata_follow(&self, path: &Path) -> io::Result<FsMetadata>;
    fn read_dir(&self, path: &Path) -> io::Result<Vec<RawEntry>>;
    fn create_dir(&self, path: &Path) -> io::Result<()>;
    /// Copy `from` — data *and* metadata — to `to` (durable-file-copy design
    /// D2). The contract:
    ///
    /// - **Exclusive create.** `to` must not exist; if it does, the call
    ///   fails with `AlreadyExists`. Nothing existing is ever overwritten or
    ///   removed to make room.
    /// - **Metadata.** Named alternate data streams, the read-only, hidden,
    ///   system and archive attributes, and the creation and last-write
    ///   times are preserved as far as the platform allows (Windows:
    ///   `CopyFileExW` plus an explicit creation-time restore; elsewhere the
    ///   last-write time and read-only state).
    /// - **Byte count.** [`CopyReport::bytes`] is the main data stream's
    ///   byte count, the quantity `on_progress` reports cumulatively.
    /// - **Length check.** Before returning `Ok`, the destination's length is
    ///   compared with `bytes`; a mismatch is an error.
    /// - **Flush.** With [`CopyOptions::flush`], the destination is flushed
    ///   to stable storage first, and a flush or close failure is an error.
    /// - **Unrestored metadata.** Metadata that could not be carried over is
    ///   listed in [`CopyReport::unrestored`]. It never fails the call and is
    ///   never silently discarded here.
    ///
    /// If `on_progress` returns [`ChunkControl::Cancel`], the copy stops, the
    /// partial destination is removed as far as the platform allows, and an
    /// `io::Error` of kind `Interrupted` is returned (the caller
    /// distinguishes cancellation from a genuine I/O failure by that kind).
    fn copy_file(&self, from: &Path, to: &Path, opts: &CopyOptions, on_progress: &mut dyn FnMut(u64) -> ChunkControl) -> io::Result<CopyReport>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn remove_dir(&self, path: &Path) -> io::Result<()>;
    /// Still on the seam after `copy_file` took over metadata restore: the
    /// worker clears a target's read-only attribute before an overwrite and
    /// a source's before a move removes it.
    fn set_readonly(&self, path: &Path, readonly: bool) -> io::Result<()>;
    /// Whether the volume holding `path` supports named alternate data
    /// streams (Windows: `GetVolumeInformationW`'s `FILE_NAMED_STREAMS`
    /// flag). The copy engine asks once per job; an error is treated as
    /// "supported" so genuine per-file stream failures still surface
    /// individually (error-diagnostics design D5).
    fn supports_named_streams(&self, path: &Path) -> io::Result<bool>;
    /// Whether `a` and `b` reside on the same volume — governs whether a
    /// move is an instant rename or a copy-then-delete.
    fn same_volume(&self, a: &Path, b: &Path) -> io::Result<bool>;
}

pub(crate) fn is_cancelled(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::Interrupted
}

fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "job cancelled")
}

// ---------------------------------------------------------------------
// RealFs
// ---------------------------------------------------------------------

/// The real filesystem. Windows builds use `MetadataExt`/`FindFirstStreamW`
/// for identity, reparse-point, and ADS support; non-Windows builds fall
/// back to a best-effort implementation (no ADS, identity derived from
/// inode+device where available) so the workspace still builds off-Windows.
#[derive(Debug, Default, Clone, Copy)]
pub struct RealFs;

impl RealFs {
    pub fn new() -> Self {
        RealFs
    }
}

/// The one way [`RealFs`] methods obtain an OS path from a caller's path.
/// `to_extended_path` leaves a relative path relative, and Windows then
/// resolves it against the process's current directory — which FileCommand
/// never changes from the launch directory — so a relative path reaching
/// `RealFs` is always an upstream bug: fail loudly in debug builds and
/// tests instead of silently acting on the wrong directory
/// (relative-destination-resolution design D4). The assertion lives at this
/// boundary rather than inside `to_extended_path` because
/// `shell::spawn_safe_path` legitimately routes relative program names such
/// as `cmd.exe` through the latter once they approach `MAX_PATH`.
fn ext(path: &Path) -> PathBuf {
    debug_assert!(path.is_absolute(), "fs_ops received a relative path: {path:?}");
    to_extended_path(path)
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;

    fn reparse_kind_from_metadata(md: &std::fs::Metadata, is_reparse: bool) -> ReparseKind {
        if !is_reparse {
            return ReparseKind::None;
        }
        // std::fs::Metadata does not expose the reparse tag directly; we
        // conservatively distinguish symlinks (which `file_type().is_symlink()`
        // reports) from other reparse points (junctions and unknown tags).
        if md.file_type().is_symlink() {
            ReparseKind::Symlink
        } else {
            ReparseKind::Junction
        }
    }

    fn to_fs_metadata(md: std::fs::Metadata) -> FsMetadata {
        let attrs = md.file_attributes();
        let is_reparse = attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0;
        FsMetadata {
            // The attribute bit, not `md.is_dir()`: the latter is false for
            // every name-surrogate reparse point, so a junction's no-follow
            // metadata would claim it is a file and the reparse-leaf removal
            // would call `DeleteFileW` on a directory object, which Windows
            // refuses with "Access is denied" (move-reparse-point-safety
            // design D4). For non-reparse entries, and for followed
            // metadata, the two agree.
            is_dir: attrs & FILE_ATTRIBUTE_DIRECTORY != 0,
            reparse: reparse_kind_from_metadata(&md, is_reparse),
            size: md.file_size(),
            modified: md.modified().ok(),
            readonly: attrs & FILE_ATTRIBUTE_READONLY != 0,
            file_id: None,
        }
    }

    impl super::FileSystem for super::RealFs {
        fn metadata(&self, path: &Path) -> io::Result<FsMetadata> {
            let p = ext(path);
            let mut out = to_fs_metadata(std::fs::symlink_metadata(&p)?);
            out.file_id = super::identity::file_id_via_handle(&p, false);
            Ok(out)
        }

        fn metadata_follow(&self, path: &Path) -> io::Result<FsMetadata> {
            let p = ext(path);
            let mut out = to_fs_metadata(std::fs::metadata(&p)?);
            out.file_id = super::identity::file_id_via_handle(&p, true);
            Ok(out)
        }

        fn read_dir(&self, path: &Path) -> io::Result<Vec<RawEntry>> {
            let p = ext(path);
            let mut out = Vec::new();
            for entry in std::fs::read_dir(&p)? {
                let entry = entry?;
                let md = entry.metadata()?;
                out.push(RawEntry { name: entry.file_name(), metadata: to_fs_metadata(md) });
            }
            Ok(out)
        }

        fn create_dir(&self, path: &Path) -> io::Result<()> {
            std::fs::create_dir(ext(path))
        }

        fn copy_file(&self, from: &Path, to: &Path, opts: &CopyOptions, on_progress: &mut dyn FnMut(u64) -> ChunkControl) -> io::Result<CopyReport> {
            super::copy_ex::copy_file(&ext(from), &ext(to), opts, on_progress)
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            std::fs::rename(ext(from), ext(to))
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            std::fs::remove_file(ext(path))
        }

        fn remove_dir(&self, path: &Path) -> io::Result<()> {
            std::fs::remove_dir(ext(path))
        }

        fn set_readonly(&self, path: &Path, readonly: bool) -> io::Result<()> {
            let p = ext(path);
            let mut perms = std::fs::metadata(&p)?.permissions();
            perms.set_readonly(readonly);
            std::fs::set_permissions(&p, perms)
        }

        fn supports_named_streams(&self, path: &Path) -> io::Result<bool> {
            super::volume_caps::supports_named_streams(&ext(path))
        }

        fn same_volume(&self, a: &Path, b: &Path) -> io::Result<bool> {
            let ma = self.metadata_follow(a)?;
            let mb = self.metadata_follow(b)?;
            match (ma.file_id, mb.file_id) {
                (Some(ida), Some(idb)) => Ok(ida.volume_serial == idb.volume_serial),
                _ => Ok(false),
            }
        }
    }
}

#[cfg(windows)]
mod copy_ex {
    //! `RealFs::copy_file` on Windows: `CopyFileExW` for the data, every
    //! named stream, the attributes and the last-write time, followed by an
    //! explicit creation-time restore, a length check, an optional flush and
    //! a post-copy stream check (durable-file-copy design D3, D4). The Win32
    //! prototypes and constants come from `windows-sys` (ci-and-build-tooling
    //! design D6); only the progress-routine *callback* is defined here, with
    //! the signature `LPPROGRESS_ROUTINE` expects.
    use super::{cancelled_error, ChunkControl, CopyOptions, CopyReport, Unrestored, UnrestoredKind};
    use core::ffi::c_void;
    use std::fs::FileTimes;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::{FileTimesExt, OpenOptionsExt};
    use std::path::Path;
    use windows_sys::Win32::Foundation::{ERROR_REQUEST_ABORTED, HANDLE};
    use windows_sys::Win32::Storage::FileSystem::{CopyFileExW, FILE_WRITE_ATTRIBUTES, LPPROGRESS_ROUTINE_CALLBACK_REASON};
    use windows_sys::Win32::System::WindowsProgramming::{
        COPY_FILE_ALLOW_DECRYPTED_DESTINATION, COPY_FILE_FAIL_IF_EXISTS, PROGRESS_CANCEL, PROGRESS_CONTINUE,
    };

    /// Everything the progress routine needs, reached through `lpData`.
    struct ProgressState<'a> {
        on_progress: &'a mut dyn FnMut(u64) -> ChunkControl,
        /// Cumulative bytes of stream 1, the main data stream. ADS bytes
        /// never move the gauge past the planned total.
        stream1_bytes: u64,
        /// A panic caught inside the routine, to be re-raised once
        /// `CopyFileExW` has returned: unwinding across the FFI boundary is
        /// undefined behavior.
        panic: Option<Box<dyn std::any::Any + Send>>,
    }

    /// The `LPPROGRESS_ROUTINE` callback `CopyFileExW` invokes; its
    /// signature matches the `windows-sys` function-pointer type exactly.
    unsafe extern "system" fn progress_routine(
        _total_file_size: i64,
        _total_bytes_transferred: i64,
        _stream_size: i64,
        stream_bytes_transferred: i64,
        stream_number: u32,
        _callback_reason: LPPROGRESS_ROUTINE_CALLBACK_REASON,
        _source_file: HANDLE,
        _destination_file: HANDLE,
        data: *const c_void,
    ) -> u32 {
        // SAFETY: `data` is the `&mut ProgressState` `copy_file` handed to
        // `CopyFileExW`, which outlives the call; `CopyFileExW` invokes the
        // routine synchronously on the calling thread, so no one else
        // touches the state while this runs.
        let state = unsafe { &mut *(data as *mut ProgressState<'_>) };
        if state.panic.is_some() {
            return PROGRESS_CANCEL;
        }
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if stream_number == 1 {
                state.stream1_bytes = stream_bytes_transferred.max(0) as u64;
            }
            // Polled on every callback, including CALLBACK_STREAM_SWITCH, so
            // cancellation is observed at the OS's chunk cadence.
            (state.on_progress)(state.stream1_bytes)
        }));
        match outcome {
            Ok(ChunkControl::Continue) => PROGRESS_CONTINUE,
            Ok(ChunkControl::Cancel) => PROGRESS_CANCEL,
            Err(payload) => {
                state.panic = Some(payload);
                PROGRESS_CANCEL
            }
        }
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }

    fn length_mismatch(expected: u64, actual: u64) -> io::Error {
        io::Error::other(format!("destination length {actual} does not match the {expected} bytes copied"))
    }

    pub fn copy_file(from: &Path, to: &Path, opts: &CopyOptions, on_progress: &mut dyn FnMut(u64) -> ChunkControl) -> io::Result<CopyReport> {
        // Followed, like `CopyFileExW` itself (no COPY_FILE_COPY_SYMLINK): a
        // file symlink is copied as its target's content.
        let src_md = std::fs::metadata(from)?;
        let wide_from = wide(from);
        let wide_to = wide(to);
        let mut state = ProgressState { on_progress, stream1_bytes: 0, panic: None };
        // SAFETY: both path buffers are null-terminated and outlive the
        // call; `state` is a live local that `progress_routine` reads back
        // through `data` only while `CopyFileExW` runs; the routine never
        // unwinds (it catches panics and stores them in `state`).
        let ok = unsafe {
            CopyFileExW(
                wide_from.as_ptr(),
                wide_to.as_ptr(),
                Some(progress_routine),
                &mut state as *mut ProgressState<'_> as *const c_void,
                std::ptr::null_mut(),
                COPY_FILE_FAIL_IF_EXISTS | COPY_FILE_ALLOW_DECRYPTED_DESTINATION,
            )
        };
        // Taken before anything else can clobber it.
        let failure = if ok == 0 { Some(io::Error::last_os_error()) } else { None };
        if let Some(payload) = state.panic.take() {
            std::panic::resume_unwind(payload);
        }
        if let Some(e) = failure {
            // A cancelled copy fails with ERROR_REQUEST_ABORTED and
            // `CopyFileExW` has already deleted its partial destination.
            return Err(if e.raw_os_error() == Some(ERROR_REQUEST_ABORTED as i32) { cancelled_error() } else { e });
        }
        let bytes = state.stream1_bytes;
        let mut unrestored = Vec::new();

        // Creation time: not preserved by `CopyFileExW`. FILE_WRITE_ATTRIBUTES
        // opens even a read-only copy. Without `flush`, the length check runs
        // on this handle too.
        match std::fs::OpenOptions::new().access_mode(FILE_WRITE_ATTRIBUTES).open(to) {
            Ok(file) => {
                match src_md.created() {
                    Ok(created) => {
                        if let Err(e) = file.set_times(FileTimes::new().set_created(created)) {
                            unrestored.push(Unrestored::new(UnrestoredKind::CreatedTime, e));
                        }
                    }
                    Err(e) => unrestored.push(Unrestored::new(UnrestoredKind::CreatedTime, e)),
                }
                if !opts.flush {
                    let len = file.metadata()?.len();
                    if len != bytes {
                        return Err(length_mismatch(bytes, len));
                    }
                }
            }
            Err(e) => {
                unrestored.push(Unrestored::new(UnrestoredKind::CreatedTime, e));
                if !opts.flush {
                    let len = std::fs::metadata(to)?.len();
                    if len != bytes {
                        return Err(length_mismatch(bytes, len));
                    }
                }
            }
        }

        // Flush: `FlushFileBuffers` needs a write handle, and a read-only
        // file cannot be opened for writing, so the bit is cleared around the
        // flush and restored afterwards (design D3). A flush, length or
        // close failure fails the copy; a failure to restore read-only does
        // not.
        if opts.flush {
            let was_readonly = std::fs::metadata(to)?.permissions().readonly();
            if was_readonly {
                let mut perms = std::fs::metadata(to)?.permissions();
                // Windows-only module: clearing read-only toggles exactly the
                // FILE_ATTRIBUTE_READONLY bit (the lint is about Unix modes).
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(false);
                std::fs::set_permissions(to, perms)?;
            }
            let flushed = (|| -> io::Result<()> {
                let file = std::fs::OpenOptions::new().write(true).open(to)?;
                let len = file.metadata()?.len();
                if len != bytes {
                    return Err(length_mismatch(bytes, len));
                }
                file.sync_all()?;
                drop(file);
                Ok(())
            })();
            if was_readonly {
                let mut perms = std::fs::metadata(to)?.permissions();
                perms.set_readonly(true);
                if let Err(e) = std::fs::set_permissions(to, perms) {
                    unrestored.push(Unrestored::new(UnrestoredKind::ReadOnly, e));
                }
            }
            flushed?;
        }

        // Named streams: `CopyFileExW` copies them itself but is not
        // documented to report one it could not create, so when the source
        // has any, the copy's list is compared afterwards. Files without
        // named streams — the common case — pay one `FindFirstStreamW`.
        match super::stream_enum::list_streams(from) {
            Ok(src_streams) if !src_streams.is_empty() => match super::stream_enum::list_streams(to) {
                Ok(dst_streams) => {
                    for name in src_streams {
                        if !dst_streams.contains(&name) {
                            unrestored.push(Unrestored::new(UnrestoredKind::Stream(name), "not present on the copy"));
                        }
                    }
                }
                Err(e) => unrestored.push(Unrestored::new(UnrestoredKind::StreamListing, e)),
            },
            Ok(_) => {}
            Err(e) => unrestored.push(Unrestored::new(UnrestoredKind::StreamListing, e)),
        }

        Ok(CopyReport { bytes, unrestored })
    }
}

#[cfg(not(windows))]
mod portable_impl {
    use super::*;

    fn to_fs_metadata(md: std::fs::Metadata) -> FsMetadata {
        let is_symlink = md.file_type().is_symlink();
        FsMetadata {
            // Deliberately `md.is_dir()`, unlike the Windows implementation:
            // a Unix symlink is never a directory object, even when it
            // points at a directory, and it is removed with `unlink`
            // (`remove_file`); `rmdir` on it fails with `ENOTDIR`
            // (move-reparse-point-safety design D4).
            is_dir: md.is_dir(),
            reparse: if is_symlink { ReparseKind::Symlink } else { ReparseKind::None },
            size: if md.is_dir() { 0 } else { md.len() },
            modified: md.modified().ok(),
            readonly: md.permissions().readonly(),
            file_id: file_id_from_metadata(&md),
        }
    }

    #[cfg(unix)]
    fn file_id_from_metadata(md: &std::fs::Metadata) -> Option<FileId> {
        use std::os::unix::fs::MetadataExt;
        Some(FileId { volume_serial: md.dev(), file_index: md.ino() })
    }

    #[cfg(not(unix))]
    fn file_id_from_metadata(_md: &std::fs::Metadata) -> Option<FileId> {
        None
    }

    impl super::FileSystem for super::RealFs {
        fn metadata(&self, path: &Path) -> io::Result<FsMetadata> {
            Ok(to_fs_metadata(std::fs::symlink_metadata(ext(path))?))
        }

        fn metadata_follow(&self, path: &Path) -> io::Result<FsMetadata> {
            Ok(to_fs_metadata(std::fs::metadata(ext(path))?))
        }

        fn read_dir(&self, path: &Path) -> io::Result<Vec<RawEntry>> {
            let mut out = Vec::new();
            for entry in std::fs::read_dir(ext(path))? {
                let entry = entry?;
                out.push(RawEntry { name: entry.file_name(), metadata: to_fs_metadata(entry.metadata()?) });
            }
            Ok(out)
        }

        fn create_dir(&self, path: &Path) -> io::Result<()> {
            std::fs::create_dir(ext(path))
        }

        /// The manual read/write loop (durable-file-copy design D8):
        /// exclusive create, chunked copy with cancel handling, last-write
        /// time on the open handle, optional `sync_all`, a length check, and
        /// read-only applied after close. Creation time, extended attributes
        /// and ADS are not exposed portably and are not attempted.
        fn copy_file(&self, from: &Path, to: &Path, opts: &CopyOptions, on_progress: &mut dyn FnMut(u64) -> ChunkControl) -> io::Result<CopyReport> {
            use std::io::{Read, Write};
            let from = ext(from);
            let to = ext(to);
            let src_md = std::fs::metadata(&from)?;
            let mut src = std::fs::File::open(&from)?;
            let mut dst = std::fs::OpenOptions::new().write(true).create_new(true).open(&to)?;
            let mut buf = vec![0u8; opts.chunk_bytes.max(4096)];
            let mut total = 0u64;
            loop {
                let n = src.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                dst.write_all(&buf[..n])?;
                total += n as u64;
                if on_progress(total) == ChunkControl::Cancel {
                    drop(dst);
                    let _ = std::fs::remove_file(&to);
                    return Err(super::cancelled_error());
                }
            }
            let mut unrestored = Vec::new();
            match src_md.modified() {
                Ok(modified) => {
                    if let Err(e) = dst.set_modified(modified) {
                        unrestored.push(Unrestored::new(UnrestoredKind::ModifiedTime, e));
                    }
                }
                Err(e) => unrestored.push(Unrestored::new(UnrestoredKind::ModifiedTime, e)),
            }
            if opts.flush {
                dst.sync_all()?;
            }
            let len = dst.metadata()?.len();
            if len != total {
                return Err(io::Error::other(format!("destination length {len} does not match the {total} bytes copied")));
            }
            drop(dst);
            if src_md.permissions().readonly() {
                let mut perms = std::fs::metadata(&to)?.permissions();
                perms.set_readonly(true);
                if let Err(e) = std::fs::set_permissions(&to, perms) {
                    unrestored.push(Unrestored::new(UnrestoredKind::ReadOnly, e));
                }
            }
            Ok(CopyReport { bytes: total, unrestored })
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            std::fs::rename(ext(from), ext(to))
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            std::fs::remove_file(ext(path))
        }

        fn remove_dir(&self, path: &Path) -> io::Result<()> {
            std::fs::remove_dir(ext(path))
        }

        fn set_readonly(&self, path: &Path, readonly: bool) -> io::Result<()> {
            let p = ext(path);
            let mut perms = std::fs::metadata(&p)?.permissions();
            perms.set_readonly(readonly);
            std::fs::set_permissions(&p, perms)
        }

        fn supports_named_streams(&self, _path: &Path) -> io::Result<bool> {
            // ADS is Windows-only; `copy_file` never reports a stream here,
            // so this answer never produces a warning off-Windows.
            Ok(false)
        }

        fn same_volume(&self, a: &Path, b: &Path) -> io::Result<bool> {
            let ma = self.metadata_follow(a)?;
            let mb = self.metadata_follow(b)?;
            match (ma.file_id, mb.file_id) {
                (Some(ida), Some(idb)) => Ok(ida.volume_serial == idb.volume_serial),
                _ => Ok(false),
            }
        }
    }
}

#[cfg(windows)]
mod volume_caps {
    //! `GetVolumeInformationW` for the per-volume `FILE_NAMED_STREAMS`
    //! capability flag, used by the copy engine to detect destinations with
    //! no ADS support at all (FAT32/exFAT) and report one aggregated warning
    //! per job instead of one per file (error-diagnostics design D5). The
    //! prototype and the flag come from `windows-sys`.
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW;
    use windows_sys::Win32::System::SystemServices::FILE_NAMED_STREAMS;

    /// Whether the volume holding `path` supports named streams.
    /// `GetVolumeInformationW` wants the volume root (`C:\` or a UNC share,
    /// trailing backslash included), which is the path's longest — i.e.
    /// last — ancestor.
    pub fn supports_named_streams(path: &Path) -> io::Result<bool> {
        let root = path.ancestors().last().unwrap_or(path);
        let wide: Vec<u16> = root.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let mut flags = 0u32;
        // SAFETY: `wide` is null-terminated and outlives the call; the only
        // non-null out pointer is `flags`, a live local.
        let ok = unsafe {
            GetVolumeInformationW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut flags,
                std::ptr::null_mut(),
                0,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(flags & FILE_NAMED_STREAMS != 0)
    }
}

#[cfg(windows)]
mod identity {
    //! Volume-serial + file-index identity via `GetFileInformationByHandle`
    //! (through `windows-sys`). `std::os::windows::fs::MetadataExt::file_index`
    //! / `volume_serial_number` remain gated behind the unstable
    //! `windows_by_handle` feature, so this crate queries the same
    //! `BY_HANDLE_FILE_INFORMATION` data directly instead of depending on it.
    use super::FileId;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    /// No data access at all: metadata only, so a file opened for exclusive
    /// writing elsewhere can still be identified.
    const NO_ACCESS: u32 = 0;

    /// Resolve `path`'s volume-serial + file-index identity. `follow`
    /// controls whether a reparse point at `path` is traversed
    /// (`GetFileInformationByHandle` on the target) or not (identity of the
    /// link itself). Returns `None` if the path can't be opened at all
    /// (e.g. permission denied) — callers treat that as "identity unknown."
    pub fn file_id_via_handle(path: &Path, follow: bool) -> Option<FileId> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let flags = FILE_FLAG_BACKUP_SEMANTICS | if follow { 0 } else { FILE_FLAG_OPEN_REPARSE_POINT };
        // SAFETY: `wide` is NUL-terminated and outlives the calls; a null
        // security-attributes pointer and a null template handle are
        // permitted by `CreateFileW`; `info` is a live, zeroed out-struct
        // (`BY_HANDLE_FILE_INFORMATION` is plain data, valid when zeroed);
        // the handle is checked against `INVALID_HANDLE_VALUE` before use and
        // closed on every path after it, so it is never used after close.
        unsafe {
            let handle = CreateFileW(
                wide.as_ptr(),
                NO_ACCESS,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                flags,
                std::ptr::null_mut(),
            );
            if handle == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut info: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
            let ok = GetFileInformationByHandle(handle, &mut info);
            CloseHandle(handle);
            if ok == 0 {
                return None;
            }
            Some(FileId {
                volume_serial: info.dwVolumeSerialNumber as u64,
                file_index: ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
            })
        }
    }
}

#[cfg(windows)]
mod stream_enum {
    //! `FindFirstStreamW`/`FindNextStreamW` (available since Vista), through
    //! `windows-sys`, to enumerate Alternate Data Streams.
    use core::ffi::c_void;
    use std::ffi::OsString;
    use std::io;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::Path;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_FILE_NOT_FOUND, ERROR_HANDLE_EOF, ERROR_NOT_SUPPORTED, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{FindClose, FindFirstStreamW, FindNextStreamW, FindStreamInfoStandard, WIN32_FIND_STREAM_DATA};

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Stream names as returned by Windows, e.g. `":Zone.Identifier:$DATA"`.
    /// The unnamed default data stream (`::$DATA`) is filtered out — callers
    /// only care about *named* ADS.
    pub fn list_streams(path: &Path) -> io::Result<Vec<OsString>> {
        let wpath = wide(path);
        let mut data = WIN32_FIND_STREAM_DATA { StreamSize: 0, cStreamName: [0u16; 296] };
        let mut out = Vec::new();
        // SAFETY: `wpath` is NUL-terminated and outlives the calls; `data` is
        // a live `WIN32_FIND_STREAM_DATA`, exactly the layout the
        // `FindStreamInfoStandard` level fills, passed as the untyped
        // out-pointer the prototype takes; the handle is checked against
        // `INVALID_HANDLE_VALUE` before use and closed on every path after
        // it; `GetLastError` is read right after the failing call.
        unsafe {
            let handle = FindFirstStreamW(wpath.as_ptr(), FindStreamInfoStandard, &mut data as *mut WIN32_FIND_STREAM_DATA as *mut c_void, 0);
            if handle == INVALID_HANDLE_VALUE {
                let err = GetLastError();
                return if err == ERROR_HANDLE_EOF || err == ERROR_FILE_NOT_FOUND || err == ERROR_NOT_SUPPORTED {
                    Ok(Vec::new())
                } else {
                    Err(io::Error::from_raw_os_error(err as i32))
                };
            }
            loop {
                let name = decode_name(&data.cStreamName);
                // The unnamed default stream is reported as `::$DATA`; a
                // named one as `:<name>:$DATA`. Only the latter are ADS.
                if let Some(stripped) = name.strip_suffix(":$DATA").and_then(|s| s.strip_prefix(':')) {
                    if !stripped.is_empty() {
                        out.push(OsString::from(stripped));
                    }
                }
                if FindNextStreamW(handle, &mut data as *mut WIN32_FIND_STREAM_DATA as *mut c_void) == 0 {
                    let err = GetLastError();
                    if err != ERROR_HANDLE_EOF {
                        FindClose(handle);
                        return Err(io::Error::from_raw_os_error(err as i32));
                    }
                    break;
                }
            }
            FindClose(handle);
        }
        Ok(out)
    }

    fn decode_name(buf: &[u16; 296]) -> String {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        OsString::from_wide(&buf[..len]).to_string_lossy().into_owned()
    }
}

// ---------------------------------------------------------------------
// FakeFs — deterministic in-memory double for unit tests. Compiled for this
// crate's tests only: the release binary carries no fault-injection double
// (durable-file-copy design D9; filesystem-error-handling "The release
// binary carries no fault-injection double").
// ---------------------------------------------------------------------

#[cfg(test)]
use std::collections::BTreeMap;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(test)]
use std::sync::Mutex;

#[cfg(test)]
#[derive(Debug, Clone)]
enum FakeNode {
    File { size: u64, modified: Option<SystemTime>, readonly: bool, streams: BTreeMap<OsString, u64> },
    Dir { readonly: bool },
    Reparse { is_dir: bool, target: PathBuf, kind: ReparseKind },
}

/// Which fs-trait operation an [`Injection`] applies to.
///
/// Copy-related injections (`CopyAfterChunks`, `Flush`, `SetModified`,
/// `SetReadonly`, `CopyStream`) are keyed on the copy's *final target* path
/// as well as on the destination actually passed to `copy_file`, so a test
/// can inject on `/dst/a.txt` without knowing that the worker copies into
/// `/dst/a.txt.~fc-partial` first (durable-file-copy design D1, D10).
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectableOp {
    Metadata,
    ReadDir,
    CreateDir,
    /// Fails a copy after the given number of chunks have already succeeded
    /// (simulates disk-full mid-copy). The partial destination node is left
    /// in place, as on a real filesystem.
    CopyAfterChunks(u64),
    /// Fails a `flush: true` copy after its last chunk, with the destination
    /// node fully written (durable-file-copy design D10).
    Flush,
    Rename,
    RemoveFile,
    RemoveDir,
    SetReadonly,
    /// Metadata-restore fault injection (error-diagnostics task 5.4): each
    /// turns the corresponding piece of metadata into an `Unrestored` entry
    /// of the copy's report. `ListStreams` keys on the *source* path.
    SetModified,
    ListStreams,
    CopyStream,
}

#[cfg(test)]
#[derive(Debug, Clone)]
struct Injection {
    op: InjectableOp,
    path: PathBuf,
    kind: io::ErrorKind,
}

/// An in-memory filesystem double. Paths are tracked as normalized
/// `PathBuf`s (via [`super::path::lexically_normalize`]); volumes are
/// modeled by an explicit per-path prefix so cross-volume-move tests don't
/// need real multi-volume hardware — see [`FakeFs::set_volume`].
#[cfg(test)]
pub struct FakeFs {
    nodes: Mutex<BTreeMap<PathBuf, FakeNode>>,
    volumes: Mutex<BTreeMap<PathBuf, u64>>,
    ids: Mutex<BTreeMap<PathBuf, u64>>,
    next_id: AtomicU64,
    injections: Mutex<Vec<Injection>>,
    /// Whether every volume answers `supports_named_streams` with `true`
    /// (the default). Flipped to model a FAT32/exFAT destination
    /// (error-diagnostics design D5).
    named_streams: AtomicBool,
}

#[cfg(test)]
impl Default for FakeFs {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl FakeFs {
    pub fn new() -> Self {
        FakeFs {
            nodes: Mutex::new(BTreeMap::new()),
            volumes: Mutex::new(BTreeMap::new()),
            ids: Mutex::new(BTreeMap::new()),
            next_id: AtomicU64::new(1),
            injections: Mutex::new(Vec::new()),
            named_streams: AtomicBool::new(true),
        }
    }

    fn norm(path: &Path) -> PathBuf {
        super::path::lexically_normalize(path)
    }

    pub fn add_dir(&self, path: &Path) {
        self.nodes.lock().unwrap().insert(Self::norm(path), FakeNode::Dir { readonly: false });
    }

    pub fn add_file(&self, path: &Path, size: u64) {
        self.nodes.lock().unwrap().insert(Self::norm(path), FakeNode::File { size, modified: None, readonly: false, streams: BTreeMap::new() });
    }

    pub fn add_file_with_stream(&self, path: &Path, size: u64, stream_name: &str, stream_size: u64) {
        let mut streams = BTreeMap::new();
        streams.insert(OsString::from(stream_name), stream_size);
        self.nodes.lock().unwrap().insert(Self::norm(path), FakeNode::File { size, modified: None, readonly: false, streams });
    }

    pub fn add_reparse_point(&self, path: &Path, is_dir: bool, target: &Path, kind: ReparseKind) {
        self.nodes.lock().unwrap().insert(Self::norm(path), FakeNode::Reparse { is_dir, target: Self::norm(target), kind });
    }

    /// Assign an explicit volume tag to everything under `prefix`. Paths
    /// with no matching prefix default to volume `0`.
    pub fn set_volume(&self, prefix: &Path, volume: u64) {
        self.volumes.lock().unwrap().insert(Self::norm(prefix), volume);
    }

    /// Test toggle for destinations without ADS support (error-diagnostics
    /// design D5): with `false`, the copy engine skips per-file stream
    /// copies and records one job-level warning.
    pub fn set_named_streams_supported(&self, supported: bool) {
        self.named_streams.store(supported, Ordering::SeqCst);
    }

    fn volume_of(&self, path: &Path) -> u64 {
        let p = Self::norm(path);
        let volumes = self.volumes.lock().unwrap();
        let mut best: Option<(&PathBuf, &u64)> = None;
        for (prefix, vol) in volumes.iter() {
            if p.starts_with(prefix) && best.map(|(b, _)| prefix.components().count() > b.components().count()).unwrap_or(true) {
                best = Some((prefix, vol));
            }
        }
        best.map(|(_, v)| *v).unwrap_or(0)
    }

    fn id_of(&self, path: &Path) -> FileId {
        let p = Self::norm(path);
        let mut ids = self.ids.lock().unwrap();
        let index = *ids.entry(p.clone()).or_insert_with(|| self.next_id.fetch_add(1, Ordering::SeqCst));
        FileId { volume_serial: self.volume_of(&p), file_index: index }
    }

    pub fn inject(&self, op: InjectableOp, path: &Path, kind: io::ErrorKind) {
        self.injections.lock().unwrap().push(Injection { op, path: Self::norm(path), kind });
    }

    fn take_injection(&self, op: InjectableOp, path: &Path) -> Option<io::ErrorKind> {
        let p = Self::norm(path);
        let mut injections = self.injections.lock().unwrap();
        if let Some(idx) = injections.iter().position(|i| i.op == op && i.path == p) {
            return Some(injections.remove(idx).kind);
        }
        None
    }

    /// The paths a copy-related injection may be keyed on for a copy into
    /// `to`: `to` itself, and — when `to` is a worker temporary
    /// (`<name>.~fc-partial[-N]`) — the final target it stands for.
    fn copy_injection_keys(to: &Path) -> Vec<PathBuf> {
        let to = Self::norm(to);
        let mut keys = vec![to.clone()];
        if let (Some(parent), Some(name)) = (to.parent(), to.file_name()) {
            let name = name.to_string_lossy();
            if let Some(cut) = name.find(super::worker::PARTIAL_SUFFIX) {
                keys.push(parent.join(&name[..cut]));
            }
        }
        keys
    }

    /// Like [`Self::take_injection`], for an `op` that applies to a copy
    /// into `to` (see [`InjectableOp`]).
    fn take_copy_injection(&self, op: InjectableOp, to: &Path) -> Option<io::ErrorKind> {
        let keys = Self::copy_injection_keys(to);
        let mut injections = self.injections.lock().unwrap();
        if let Some(idx) = injections.iter().position(|i| i.op == op && keys.contains(&i.path)) {
            return Some(injections.remove(idx).kind);
        }
        None
    }

    /// Test helper: pin a file's last-write time (the field `copy_file`
    /// carries over). Not on the trait any more (durable-file-copy design
    /// D2).
    pub fn set_modified(&self, path: &Path, modified: SystemTime) -> io::Result<()> {
        let p = self.resolve_ancestors(path);
        let mut nodes = self.nodes.lock().unwrap();
        match nodes.get_mut(&p) {
            Some(FakeNode::File { modified: m, .. }) => {
                *m = Some(modified);
                Ok(())
            }
            _ => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }

    /// Test helper: the named streams modeled on a file, for asserting what
    /// `copy_file` carried over. Not on the trait any more (durable-file-copy
    /// design D2).
    pub fn list_streams(&self, path: &Path) -> io::Result<Vec<OsString>> {
        let p = self.resolve_ancestors(path);
        let nodes = self.nodes.lock().unwrap();
        match nodes.get(&p) {
            Some(FakeNode::File { streams, .. }) => Ok(streams.keys().cloned().collect()),
            Some(_) => Ok(Vec::new()),
            None => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }

    /// Resolve `path` the way real NTFS resolves a path through junctions:
    /// every *ancestor* component is transparently redirected if it's a
    /// reparse point, but the final component itself is left as-is (so
    /// callers can still see that the final component is a reparse point).
    ///
    /// Every path-taking operation — not only `metadata` — resolves through
    /// here, so a mutation addressed *through* a link lands on the target
    /// side exactly as `DeleteFileW`/`RemoveDirectoryW` would, while a
    /// mutation addressed *at* a link acts on the link node itself
    /// (move-reparse-point-safety design D5). Without that, a test could
    /// not reproduce a move deleting through a junction.
    fn resolve_ancestors(&self, path: &Path) -> PathBuf {
        let p = Self::norm(path);
        match (p.parent(), p.file_name()) {
            (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => self.resolve_full(parent).join(name),
            _ => p,
        }
    }

    /// Like [`Self::resolve_ancestors`], but also follows the final
    /// component if it is itself a reparse point (repeatedly, for chains).
    fn resolve_full(&self, path: &Path) -> PathBuf {
        let mut p = self.resolve_ancestors(path);
        let mut hops = 0;
        loop {
            let target = match self.nodes.lock().unwrap().get(&p) {
                Some(FakeNode::Reparse { target, .. }) => Some(target.clone()),
                _ => None,
            };
            match target {
                Some(t) if hops < 32 => {
                    p = self.resolve_ancestors(&t);
                    hops += 1;
                }
                _ => break,
            }
        }
        p
    }

    fn node_metadata(node: &FakeNode, id: FileId) -> FsMetadata {
        match node {
            FakeNode::File { size, modified, readonly, .. } => {
                FsMetadata { is_dir: false, reparse: ReparseKind::None, size: *size, modified: *modified, readonly: *readonly, file_id: Some(id) }
            }
            FakeNode::Dir { readonly } => {
                FsMetadata { is_dir: true, reparse: ReparseKind::None, size: 0, modified: None, readonly: *readonly, file_id: Some(id) }
            }
            FakeNode::Reparse { is_dir, kind, .. } => {
                FsMetadata { is_dir: *is_dir, reparse: *kind, size: 0, modified: None, readonly: false, file_id: Some(id) }
            }
        }
    }
}

#[cfg(test)]
impl FileSystem for FakeFs {
    fn metadata(&self, path: &Path) -> io::Result<FsMetadata> {
        if let Some(kind) = self.take_injection(InjectableOp::Metadata, path) {
            return Err(io::Error::from(kind));
        }
        // Ancestors are transparently redirected (like real NTFS junction
        // traversal); the final component is left as-is (lstat-like).
        let p = self.resolve_ancestors(path);
        let nodes = self.nodes.lock().unwrap();
        let node = nodes.get(&p).ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        Ok(Self::node_metadata(node, self.id_of(&p)))
    }

    fn metadata_follow(&self, path: &Path) -> io::Result<FsMetadata> {
        let p = self.resolve_full(path);
        let nodes = self.nodes.lock().unwrap();
        let node = nodes.get(&p).ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        Ok(Self::node_metadata(node, self.id_of(&p)))
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<RawEntry>> {
        if let Some(kind) = self.take_injection(InjectableOp::ReadDir, path) {
            return Err(io::Error::from(kind));
        }
        // A directory-type reparse point is always transparently followed
        // when listing its contents, so resolve fully here.
        let p = self.resolve_full(path);
        let nodes = self.nodes.lock().unwrap();
        let mut out = Vec::new();
        for (candidate, node) in nodes.iter() {
            if candidate.parent() == Some(p.as_path()) {
                let name = candidate.file_name().unwrap().to_os_string();
                out.push(RawEntry { name, metadata: Self::node_metadata(node, self.id_of(candidate)) });
            }
        }
        Ok(out)
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        if let Some(kind) = self.take_injection(InjectableOp::CreateDir, path) {
            return Err(io::Error::from(kind));
        }
        let p = self.resolve_ancestors(path);
        let mut nodes = self.nodes.lock().unwrap();
        if nodes.contains_key(&p) {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists));
        }
        nodes.insert(p, FakeNode::Dir { readonly: false });
        Ok(())
    }

    /// Durable-file-copy design D10: the destination node exists from the
    /// first chunk and grows per chunk, so an injected failure or a cancel
    /// leaves a partial node exactly as a real filesystem would; creation is
    /// exclusive; and the source's streams, read-only state and last-write
    /// time are carried over on success, each one divertible into
    /// `unrestored` by the matching injection.
    fn copy_file(&self, from: &Path, to: &Path, opts: &CopyOptions, on_progress: &mut dyn FnMut(u64) -> ChunkControl) -> io::Result<CopyReport> {
        // Opening a file for reading follows a file symlink, so the source
        // is resolved fully; the destination resolves its ancestors like
        // every other mutation (move-reparse-point-safety design D5).
        let src = self.resolve_full(from);
        let (size, streams, readonly, modified) = match self.nodes.lock().unwrap().get(&src) {
            Some(FakeNode::File { size, streams, readonly, modified }) => (*size, streams.clone(), *readonly, *modified),
            Some(_) => return Err(io::Error::from(io::ErrorKind::InvalidInput)),
            None => return Err(io::Error::from(io::ErrorKind::NotFound)),
        };
        let dest = self.resolve_ancestors(to);
        {
            let mut nodes = self.nodes.lock().unwrap();
            if nodes.contains_key(&dest) {
                return Err(io::Error::from(io::ErrorKind::AlreadyExists));
            }
            nodes.insert(dest.clone(), FakeNode::File { size: 0, modified: None, readonly: false, streams: BTreeMap::new() });
        }
        let keys = Self::copy_injection_keys(to);
        let chunk = opts.chunk_bytes.max(1) as u64;
        let mut done = 0u64;
        let mut chunk_index = 0u64;
        loop {
            let fail_after = {
                let mut injections = self.injections.lock().unwrap();
                let idx =
                    injections.iter().position(|i| matches!(i.op, InjectableOp::CopyAfterChunks(n) if n == chunk_index) && keys.contains(&i.path));
                idx.map(|idx| injections.remove(idx).kind)
            };
            if let Some(kind) = fail_after {
                return Err(io::Error::from(kind));
            }
            let step = chunk.min(size - done);
            done += step;
            chunk_index += 1;
            if let Some(FakeNode::File { size: s, .. }) = self.nodes.lock().unwrap().get_mut(&dest) {
                *s = done;
            }
            if on_progress(done) == ChunkControl::Cancel {
                return Err(cancelled_error());
            }
            if done >= size {
                break;
            }
        }
        if opts.flush {
            if let Some(kind) = self.take_copy_injection(InjectableOp::Flush, to) {
                return Err(io::Error::from(kind));
            }
        }

        let mut unrestored = Vec::new();
        if let Some(kind) = self.take_injection(InjectableOp::ListStreams, from) {
            unrestored.push(Unrestored::new(UnrestoredKind::StreamListing, io::Error::from(kind)));
        } else {
            for (name, stream_size) in streams {
                if !self.named_streams.load(Ordering::SeqCst) {
                    unrestored.push(Unrestored::new(UnrestoredKind::Stream(name), "destination volume has no named-stream support"));
                } else if let Some(kind) = self.take_copy_injection(InjectableOp::CopyStream, to) {
                    unrestored.push(Unrestored::new(UnrestoredKind::Stream(name), io::Error::from(kind)));
                } else if let Some(FakeNode::File { streams, .. }) = self.nodes.lock().unwrap().get_mut(&dest) {
                    streams.insert(name, stream_size);
                }
            }
        }
        if let Some(modified) = modified {
            if let Some(kind) = self.take_copy_injection(InjectableOp::SetModified, to) {
                unrestored.push(Unrestored::new(UnrestoredKind::ModifiedTime, io::Error::from(kind)));
            } else if let Some(FakeNode::File { modified: m, .. }) = self.nodes.lock().unwrap().get_mut(&dest) {
                *m = Some(modified);
            }
        }
        if readonly {
            if let Some(kind) = self.take_copy_injection(InjectableOp::SetReadonly, to) {
                unrestored.push(Unrestored::new(UnrestoredKind::ReadOnly, io::Error::from(kind)));
            } else if let Some(FakeNode::File { readonly: r, .. }) = self.nodes.lock().unwrap().get_mut(&dest) {
                *r = true;
            }
        }
        Ok(CopyReport { bytes: size, unrestored })
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        if let Some(kind) = self.take_injection(InjectableOp::Rename, from) {
            return Err(io::Error::from(kind));
        }
        let pf = self.resolve_ancestors(from);
        let pt = self.resolve_ancestors(to);
        let mut nodes = self.nodes.lock().unwrap();
        let node = nodes.remove(&pf).ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        nodes.insert(pt.clone(), node);
        drop(nodes);
        // Identity (including case-only renames) survives the path-key
        // change — a rename is not a new file.
        let mut ids = self.ids.lock().unwrap();
        if let Some(id) = ids.remove(&pf) {
            ids.insert(pt, id);
        }
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        if let Some(kind) = self.take_injection(InjectableOp::RemoveFile, path) {
            return Err(io::Error::from(kind));
        }
        let p = self.resolve_ancestors(path);
        let mut nodes = self.nodes.lock().unwrap();
        match nodes.get(&p) {
            Some(FakeNode::File { readonly: true, .. }) => Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            // `DeleteFileW` refuses a directory object — a real directory
            // or a directory junction/symlink — with "Access is denied"
            // (move-reparse-point-safety design D5).
            Some(FakeNode::Dir { .. }) | Some(FakeNode::Reparse { is_dir: true, .. }) => Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            Some(_) => {
                nodes.remove(&p);
                Ok(())
            }
            None => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        if let Some(kind) = self.take_injection(InjectableOp::RemoveDir, path) {
            return Err(io::Error::from(kind));
        }
        // Ancestors resolve, the final component does not: `remove_dir` on
        // a junction path removes the link node itself, and the emptiness
        // check below looks at the link's own (always empty) children, never
        // at the target's.
        let p = self.resolve_ancestors(path);
        let mut nodes = self.nodes.lock().unwrap();
        if nodes.keys().any(|k| k.parent() == Some(p.as_path())) {
            return Err(io::Error::other("directory not empty"));
        }
        nodes.remove(&p).ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        Ok(())
    }

    fn set_readonly(&self, path: &Path, readonly: bool) -> io::Result<()> {
        if let Some(kind) = self.take_injection(InjectableOp::SetReadonly, path) {
            return Err(io::Error::from(kind));
        }
        let p = self.resolve_ancestors(path);
        let mut nodes = self.nodes.lock().unwrap();
        match nodes.get_mut(&p) {
            Some(FakeNode::File { readonly: r, .. }) => *r = readonly,
            Some(FakeNode::Dir { readonly: r }) => *r = readonly,
            _ => return Err(io::Error::from(io::ErrorKind::NotFound)),
        }
        Ok(())
    }

    fn same_volume(&self, a: &Path, b: &Path) -> io::Result<bool> {
        Ok(self.volume_of(a) == self.volume_of(b))
    }

    fn supports_named_streams(&self, _path: &Path) -> io::Result<bool> {
        Ok(self.named_streams.load(Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_fs_metadata_injection_returns_requested_error() {
        let fake = FakeFs::new();
        fake.add_file(Path::new("/a.txt"), 10);
        fake.inject(InjectableOp::Metadata, Path::new("/a.txt"), io::ErrorKind::PermissionDenied);
        let err = fake.metadata(Path::new("/a.txt")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        // Injection is one-shot: the next call succeeds.
        assert!(fake.metadata(Path::new("/a.txt")).is_ok());
    }

    fn opts(chunk_bytes: usize, flush: bool) -> CopyOptions {
        CopyOptions { chunk_bytes, flush }
    }

    #[test]
    fn fake_fs_copy_disk_full_after_n_chunks_leaves_a_partial_node() {
        let fake = FakeFs::new();
        fake.add_file(Path::new("/src.bin"), 30);
        fake.inject(InjectableOp::CopyAfterChunks(2), Path::new("/dst.bin"), io::ErrorKind::StorageFull);
        let mut chunks = 0u64;
        let err = fake
            .copy_file(Path::new("/src.bin"), Path::new("/dst.bin"), &opts(10, false), &mut |_| {
                chunks += 1;
                ChunkControl::Continue
            })
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::StorageFull);
        assert_eq!(chunks, 2);
        // durable-file-copy design D10: the partial destination stays, as
        // on a real filesystem, so the worker's cleanup is testable.
        assert_eq!(fake.metadata(Path::new("/dst.bin")).unwrap().size, 20, "two chunks of ten bytes landed");
    }

    #[test]
    fn fake_fs_cancel_stops_copy_with_interrupted() {
        let fake = FakeFs::new();
        fake.add_file(Path::new("/src.bin"), 30);
        let err = fake
            .copy_file(Path::new("/src.bin"), Path::new("/dst.bin"), &opts(10, false), &mut |done| {
                if done >= 10 {
                    ChunkControl::Cancel
                } else {
                    ChunkControl::Continue
                }
            })
            .unwrap_err();
        assert!(is_cancelled(&err));
        assert_eq!(fake.metadata(Path::new("/dst.bin")).unwrap().size, 10, "a cancel leaves the partial node too");
    }

    #[test]
    fn fake_fs_copy_creates_the_destination_exclusively() {
        let fake = FakeFs::new();
        fake.add_file(Path::new("/src.bin"), 3);
        fake.add_file(Path::new("/dst.bin"), 99);
        let err = fake.copy_file(Path::new("/src.bin"), Path::new("/dst.bin"), &opts(10, false), &mut |_| ChunkControl::Continue).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fake.metadata(Path::new("/dst.bin")).unwrap().size, 99, "the existing file is untouched");
    }

    #[test]
    fn fake_fs_flush_injection_fails_a_flushed_copy_after_the_last_chunk() {
        let fake = FakeFs::new();
        fake.add_file(Path::new("/src.bin"), 30);
        fake.inject(InjectableOp::Flush, Path::new("/dst.bin"), io::ErrorKind::Other);
        let err = fake.copy_file(Path::new("/src.bin"), Path::new("/dst.bin"), &opts(10, true), &mut |_| ChunkControl::Continue).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Other);
        assert_eq!(fake.metadata(Path::new("/dst.bin")).unwrap().size, 30, "fully written, then the flush failed");
        // The same injection is ignored by an unflushed copy.
        fake.inject(InjectableOp::Flush, Path::new("/dst2.bin"), io::ErrorKind::Other);
        assert!(fake.copy_file(Path::new("/src.bin"), Path::new("/dst2.bin"), &opts(10, false), &mut |_| ChunkControl::Continue).is_ok());
    }

    #[test]
    fn fake_fs_copy_injections_key_on_the_final_target_of_a_partial_file() {
        let fake = FakeFs::new();
        fake.add_file(Path::new("/src.bin"), 30);
        fake.inject(InjectableOp::CopyAfterChunks(0), Path::new("/dst/a.bin"), io::ErrorKind::StorageFull);
        let temp = PathBuf::from(format!("/dst/a.bin{}-2", super::super::worker::PARTIAL_SUFFIX));
        let err = fake.copy_file(Path::new("/src.bin"), &temp, &opts(10, false), &mut |_| ChunkControl::Continue).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::StorageFull);
    }

    #[test]
    fn fake_fs_identity_is_stable_and_volume_aware() {
        let fake = FakeFs::new();
        fake.add_file(Path::new("/v1/a.txt"), 1);
        fake.add_file(Path::new("/v2/a.txt"), 1);
        fake.set_volume(Path::new("/v2"), 2);
        let id1 = fake.metadata(Path::new("/v1/a.txt")).unwrap().file_id.unwrap();
        let id2 = fake.metadata(Path::new("/v2/a.txt")).unwrap().file_id.unwrap();
        assert_ne!(id1.volume_serial, id2.volume_serial);
        assert!(!fake.same_volume(Path::new("/v1/a.txt"), Path::new("/v2/a.txt")).unwrap());
        assert!(fake.same_volume(Path::new("/v1/a.txt"), Path::new("/v1/a.txt")).unwrap());
    }

    #[test]
    fn fake_fs_case_only_rename_preserves_identity() {
        let fake = FakeFs::new();
        fake.add_file(Path::new("/dir/Name.txt"), 5);
        let before = fake.metadata(Path::new("/dir/Name.txt")).unwrap().file_id;
        fake.rename(Path::new("/dir/Name.txt"), Path::new("/dir/NAME.txt")).unwrap();
        let after = fake.metadata(Path::new("/dir/NAME.txt")).unwrap().file_id;
        assert_eq!(before, after);
    }

    #[test]
    fn fake_fs_readonly_blocks_remove_until_cleared() {
        let fake = FakeFs::new();
        fake.add_file(Path::new("/ro.txt"), 1);
        fake.set_readonly(Path::new("/ro.txt"), true).unwrap();
        assert_eq!(fake.remove_file(Path::new("/ro.txt")).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        fake.set_readonly(Path::new("/ro.txt"), false).unwrap();
        assert!(fake.remove_file(Path::new("/ro.txt")).is_ok());
    }

    #[test]
    fn fake_fs_reparse_point_metadata_follow_resolves_target() {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/real"));
        fake.add_file(Path::new("/real/f.txt"), 9);
        fake.add_reparse_point(Path::new("/link"), true, Path::new("/real"), ReparseKind::Junction);
        let no_follow = fake.metadata(Path::new("/link")).unwrap();
        assert!(no_follow.reparse.is_reparse_point());
        let followed = fake.metadata_follow(Path::new("/link")).unwrap();
        assert!(!followed.reparse.is_reparse_point());
        assert!(followed.is_dir);
    }

    // move-reparse-point-safety design D5: mutations resolve ancestor links
    // like NTFS, and `remove_file` refuses directory objects.

    fn junction_fixture() -> FakeFs {
        let fake = FakeFs::new();
        fake.add_dir(Path::new("/shared"));
        fake.add_file(Path::new("/shared/f.txt"), 9);
        fake.add_dir(Path::new("/work"));
        fake.add_reparse_point(Path::new("/work/link"), true, Path::new("/shared"), ReparseKind::Junction);
        fake
    }

    #[test]
    fn fake_fs_remove_file_through_ancestor_junction_removes_the_target_side_node() {
        let fake = junction_fixture();
        fake.remove_file(Path::new("/work/link/f.txt")).unwrap();
        assert!(fake.metadata(Path::new("/shared/f.txt")).is_err(), "the real file behind the link is gone, as on NTFS");
        assert!(fake.metadata(Path::new("/work/link")).is_ok(), "the link itself is untouched");
    }

    #[test]
    fn fake_fs_remove_dir_on_a_junction_path_removes_only_the_link_node() {
        let fake = junction_fixture();
        fake.remove_dir(Path::new("/work/link")).unwrap();
        assert!(fake.metadata(Path::new("/work/link")).is_err(), "the link node is gone");
        assert_eq!(fake.metadata(Path::new("/shared/f.txt")).unwrap().size, 9, "the target and its content survive");
    }

    #[test]
    fn fake_fs_remove_file_on_a_directory_junction_is_refused() {
        let fake = junction_fixture();
        let err = fake.remove_file(Path::new("/work/link")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied, "DeleteFileW on a directory object is denied");
        assert!(fake.metadata(Path::new("/work/link")).is_ok());
        fake.add_dir(Path::new("/plain"));
        assert_eq!(fake.remove_file(Path::new("/plain")).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn fake_fs_mutations_through_a_junction_land_on_the_target_side() {
        let fake = junction_fixture();
        fake.set_readonly(Path::new("/work/link/f.txt"), true).unwrap();
        assert!(fake.metadata(Path::new("/shared/f.txt")).unwrap().readonly, "set_readonly followed the ancestor link");
        fake.create_dir(Path::new("/work/link/sub")).unwrap();
        assert!(fake.metadata(Path::new("/shared/sub")).unwrap().is_dir, "create_dir followed the ancestor link");
        fake.rename(Path::new("/work/link/sub"), Path::new("/work/link/sub2")).unwrap();
        assert!(fake.metadata(Path::new("/shared/sub2")).is_ok(), "rename resolved both ends");
        fake.remove_dir(Path::new("/work/link/sub2")).unwrap();
        assert!(fake.metadata(Path::new("/shared/sub2")).is_err(), "remove_dir through the link removed the target-side dir");
    }

    #[test]
    fn fake_fs_copy_carries_streams_readonly_and_modified_over() {
        let fake = FakeFs::new();
        fake.add_file_with_stream(Path::new("/a.txt"), 5, "Zone.Identifier", 3);
        fake.set_readonly(Path::new("/a.txt"), true).unwrap();
        let modified = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        fake.set_modified(Path::new("/a.txt"), modified).unwrap();
        assert_eq!(fake.list_streams(Path::new("/a.txt")).unwrap(), vec![OsString::from("Zone.Identifier")]);

        let report = fake.copy_file(Path::new("/a.txt"), Path::new("/b.txt"), &opts(64, false), &mut |_| ChunkControl::Continue).unwrap();
        assert_eq!(report.bytes, 5);
        assert!(report.unrestored.is_empty(), "{:?}", report.unrestored);
        assert_eq!(fake.list_streams(Path::new("/b.txt")).unwrap(), vec![OsString::from("Zone.Identifier")]);
        let md = fake.metadata(Path::new("/b.txt")).unwrap();
        assert!(md.readonly);
        assert_eq!(md.modified, Some(modified));
    }

    #[test]
    fn fake_fs_metadata_injections_divert_into_the_copy_report() {
        let fake = FakeFs::new();
        fake.add_file_with_stream(Path::new("/a.txt"), 5, "Zone.Identifier", 3);
        fake.set_readonly(Path::new("/a.txt"), true).unwrap();
        fake.set_modified(Path::new("/a.txt"), SystemTime::UNIX_EPOCH).unwrap();
        fake.inject(InjectableOp::CopyStream, Path::new("/b.txt"), io::ErrorKind::PermissionDenied);
        fake.inject(InjectableOp::SetModified, Path::new("/b.txt"), io::ErrorKind::PermissionDenied);
        fake.inject(InjectableOp::SetReadonly, Path::new("/b.txt"), io::ErrorKind::PermissionDenied);

        let report = fake.copy_file(Path::new("/a.txt"), Path::new("/b.txt"), &opts(64, false), &mut |_| ChunkControl::Continue).unwrap();
        let kinds: Vec<&UnrestoredKind> = report.unrestored.iter().map(|u| &u.what).collect();
        assert_eq!(kinds, vec![&UnrestoredKind::Stream(OsString::from("Zone.Identifier")), &UnrestoredKind::ModifiedTime, &UnrestoredKind::ReadOnly]);
        assert_eq!(report.unrestored[0].reason(), format!("stream Zone.Identifier not copied: {}", io::Error::from(io::ErrorKind::PermissionDenied)));
        assert!(report.unrestored[1].reason().starts_with("modification time not restored: "));
        assert!(report.unrestored[2].reason().starts_with("read-only attribute not restored: "));
        let md = fake.metadata(Path::new("/b.txt")).unwrap();
        assert_eq!(md.size, 5, "the content still arrived");
        assert!(!md.readonly);
        assert!(fake.list_streams(Path::new("/b.txt")).unwrap().is_empty());
    }
}
