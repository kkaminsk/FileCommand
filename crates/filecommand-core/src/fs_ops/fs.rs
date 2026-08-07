//! The narrow filesystem trait seam `fs_ops` operates through. No code
//! anywhere else in `fs_ops` calls `std::fs` directly — every access goes
//! through a [`FileSystem`] implementation, routed through
//! [`crate::fs_ops::path::to_extended_path`].
//!
//! Two implementations exist: [`RealFs`] (the real, Windows-backed
//! filesystem) and [`FakeFs`] (an in-memory double for deterministic unit
//! tests, including error injection).

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
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

/// What a chunked-copy progress callback tells the caller to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkControl {
    Continue,
    Cancel,
}

/// The narrow fs-access seam. `metadata` never follows a reparse point
/// (lstat-like); `metadata_follow` always does (stat-like) — `fs_ops` needs
/// both to implement reparse-point semantics correctly.
pub trait FileSystem: Send + Sync {
    fn metadata(&self, path: &Path) -> io::Result<FsMetadata>;
    fn metadata_follow(&self, path: &Path) -> io::Result<FsMetadata>;
    fn read_dir(&self, path: &Path) -> io::Result<Vec<RawEntry>>;
    fn create_dir(&self, path: &Path) -> io::Result<()>;
    /// Copy `from` to `to` in chunks of roughly `chunk_bytes`, invoking
    /// `on_chunk` with the cumulative byte count after each chunk. Returns
    /// the total bytes copied. If `on_chunk` returns
    /// [`ChunkControl::Cancel`], the copy stops and an `io::Error` of kind
    /// `Interrupted` is returned (the caller distinguishes cancellation from
    /// a genuine I/O failure by that error kind).
    fn copy_file_chunked(
        &self,
        from: &Path,
        to: &Path,
        chunk_bytes: usize,
        on_chunk: &mut dyn FnMut(u64) -> ChunkControl,
    ) -> io::Result<u64>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn remove_dir(&self, path: &Path) -> io::Result<()>;
    fn set_readonly(&self, path: &Path, readonly: bool) -> io::Result<()>;
    fn set_modified(&self, path: &Path, modified: SystemTime) -> io::Result<()>;
    /// Alternate Data Stream names attached to `path` (Windows NTFS only).
    /// Always empty on platforms/filesystems without ADS support.
    fn list_streams(&self, path: &Path) -> io::Result<Vec<OsString>>;
    fn copy_stream_chunked(&self, from: &Path, stream: &OsStr, to: &Path, chunk_bytes: usize) -> io::Result<u64>;
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

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    const FILE_ATTRIBUTE_READONLY: u32 = 0x1;

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
            is_dir: md.is_dir(),
            reparse: reparse_kind_from_metadata(&md, is_reparse),
            size: md.file_size(),
            modified: md.modified().ok(),
            readonly: attrs & FILE_ATTRIBUTE_READONLY != 0,
            file_id: None,
        }
    }

    impl super::FileSystem for super::RealFs {
        fn metadata(&self, path: &Path) -> io::Result<FsMetadata> {
            let p = to_extended_path(path);
            let mut out = to_fs_metadata(std::fs::symlink_metadata(&p)?);
            out.file_id = super::identity::file_id_via_handle(&p, false);
            Ok(out)
        }

        fn metadata_follow(&self, path: &Path) -> io::Result<FsMetadata> {
            let p = to_extended_path(path);
            let mut out = to_fs_metadata(std::fs::metadata(&p)?);
            out.file_id = super::identity::file_id_via_handle(&p, true);
            Ok(out)
        }

        fn read_dir(&self, path: &Path) -> io::Result<Vec<RawEntry>> {
            let p = to_extended_path(path);
            let mut out = Vec::new();
            for entry in std::fs::read_dir(&p)? {
                let entry = entry?;
                let md = entry.metadata()?;
                out.push(RawEntry { name: entry.file_name(), metadata: to_fs_metadata(md) });
            }
            Ok(out)
        }

        fn create_dir(&self, path: &Path) -> io::Result<()> {
            std::fs::create_dir(to_extended_path(path))
        }

        fn copy_file_chunked(
            &self,
            from: &Path,
            to: &Path,
            chunk_bytes: usize,
            on_chunk: &mut dyn FnMut(u64) -> ChunkControl,
        ) -> io::Result<u64> {
            copy_chunked_impl(&to_extended_path(from), &to_extended_path(to), chunk_bytes, on_chunk)
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            std::fs::rename(to_extended_path(from), to_extended_path(to))
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            std::fs::remove_file(to_extended_path(path))
        }

        fn remove_dir(&self, path: &Path) -> io::Result<()> {
            std::fs::remove_dir(to_extended_path(path))
        }

        fn set_readonly(&self, path: &Path, readonly: bool) -> io::Result<()> {
            let p = to_extended_path(path);
            let mut perms = std::fs::metadata(&p)?.permissions();
            perms.set_readonly(readonly);
            std::fs::set_permissions(&p, perms)
        }

        fn set_modified(&self, path: &Path, modified: SystemTime) -> io::Result<()> {
            let p = to_extended_path(path);
            let file = std::fs::OpenOptions::new().write(true).open(&p)?;
            file.set_modified(modified)
        }

        fn list_streams(&self, path: &Path) -> io::Result<Vec<OsString>> {
            super::stream_enum::list_streams(&to_extended_path(path))
        }

        fn copy_stream_chunked(&self, from: &Path, stream: &OsStr, to: &Path, chunk_bytes: usize) -> io::Result<u64> {
            let from_stream = super::stream_enum::stream_path(&to_extended_path(from), stream);
            let to_stream = super::stream_enum::stream_path(&to_extended_path(to), stream);
            copy_chunked_impl(&from_stream, &to_stream, chunk_bytes, &mut |_| ChunkControl::Continue)
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

    fn copy_chunked_impl(
        from: &Path,
        to: &Path,
        chunk_bytes: usize,
        on_chunk: &mut dyn FnMut(u64) -> ChunkControl,
    ) -> io::Result<u64> {
        use std::io::{Read, Write};
        let mut src = std::fs::File::open(from)?;
        let mut dst = std::fs::File::create(to)?;
        let mut buf = vec![0u8; chunk_bytes.max(4096)];
        let mut total = 0u64;
        loop {
            let n = src.read(&mut buf)?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n])?;
            total += n as u64;
            if on_chunk(total) == ChunkControl::Cancel {
                drop(dst);
                let _ = std::fs::remove_file(to);
                return Err(super::cancelled_error());
            }
        }
        Ok(total)
    }
}

#[cfg(not(windows))]
mod portable_impl {
    use super::*;

    fn to_fs_metadata(md: std::fs::Metadata) -> FsMetadata {
        let is_symlink = md.file_type().is_symlink();
        FsMetadata {
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
            Ok(to_fs_metadata(std::fs::symlink_metadata(to_extended_path(path))?))
        }

        fn metadata_follow(&self, path: &Path) -> io::Result<FsMetadata> {
            Ok(to_fs_metadata(std::fs::metadata(to_extended_path(path))?))
        }

        fn read_dir(&self, path: &Path) -> io::Result<Vec<RawEntry>> {
            let mut out = Vec::new();
            for entry in std::fs::read_dir(to_extended_path(path))? {
                let entry = entry?;
                out.push(RawEntry { name: entry.file_name(), metadata: to_fs_metadata(entry.metadata()?) });
            }
            Ok(out)
        }

        fn create_dir(&self, path: &Path) -> io::Result<()> {
            std::fs::create_dir(to_extended_path(path))
        }

        fn copy_file_chunked(
            &self,
            from: &Path,
            to: &Path,
            chunk_bytes: usize,
            on_chunk: &mut dyn FnMut(u64) -> ChunkControl,
        ) -> io::Result<u64> {
            use std::io::{Read, Write};
            let mut src = std::fs::File::open(to_extended_path(from))?;
            let mut dst = std::fs::File::create(to_extended_path(to))?;
            let mut buf = vec![0u8; chunk_bytes.max(4096)];
            let mut total = 0u64;
            loop {
                let n = src.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                dst.write_all(&buf[..n])?;
                total += n as u64;
                if on_chunk(total) == ChunkControl::Cancel {
                    drop(dst);
                    let _ = std::fs::remove_file(to_extended_path(to));
                    return Err(super::cancelled_error());
                }
            }
            Ok(total)
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            std::fs::rename(to_extended_path(from), to_extended_path(to))
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            std::fs::remove_file(to_extended_path(path))
        }

        fn remove_dir(&self, path: &Path) -> io::Result<()> {
            std::fs::remove_dir(to_extended_path(path))
        }

        fn set_readonly(&self, path: &Path, readonly: bool) -> io::Result<()> {
            let p = to_extended_path(path);
            let mut perms = std::fs::metadata(&p)?.permissions();
            perms.set_readonly(readonly);
            std::fs::set_permissions(&p, perms)
        }

        fn set_modified(&self, path: &Path, modified: SystemTime) -> io::Result<()> {
            let p = to_extended_path(path);
            let file = std::fs::OpenOptions::new().write(true).open(&p)?;
            file.set_modified(modified)
        }

        fn list_streams(&self, _path: &Path) -> io::Result<Vec<OsString>> {
            Ok(Vec::new())
        }

        fn copy_stream_chunked(&self, _from: &Path, _stream: &OsStr, _to: &Path, _chunk_bytes: usize) -> io::Result<u64> {
            Ok(0)
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
mod identity {
    //! Volume-serial + file-index identity via raw `GetFileInformationByHandle`
    //! FFI. `std::os::windows::fs::MetadataExt::file_index` /
    //! `volume_serial_number` remain gated behind the unstable
    //! `windows_by_handle` feature, so this crate queries the same
    //! `BY_HANDLE_FILE_INFORMATION` data directly instead of depending on it.
    use super::FileId;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    #[allow(non_camel_case_types)]
    type HANDLE = *mut core::ffi::c_void;
    #[allow(non_camel_case_types)]
    type c_void = core::ffi::c_void;

    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[repr(C)]
    struct ByHandleFileInformation {
        file_attributes: u32,
        creation_time: FileTime,
        last_access_time: FileTime,
        last_write_time: FileTime,
        volume_serial_number: u32,
        file_size_high: u32,
        file_size_low: u32,
        number_of_links: u32,
        file_index_high: u32,
        file_index_low: u32,
    }

    const GENERIC_READ_NONE: u32 = 0;
    const FILE_SHARE_READ: u32 = 0x1;
    const FILE_SHARE_WRITE: u32 = 0x2;
    const FILE_SHARE_DELETE: u32 = 0x4;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const INVALID_HANDLE_VALUE: isize = -1;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(
            file_name: *const u16,
            desired_access: u32,
            share_mode: u32,
            security_attrs: *mut c_void,
            creation_disposition: u32,
            flags_and_attrs: u32,
            template_file: HANDLE,
        ) -> HANDLE;
        fn GetFileInformationByHandle(file: HANDLE, info: *mut ByHandleFileInformation) -> i32;
        fn CloseHandle(object: HANDLE) -> i32;
    }

    /// Resolve `path`'s volume-serial + file-index identity. `follow`
    /// controls whether a reparse point at `path` is traversed
    /// (`GetFileInformationByHandle` on the target) or not (identity of the
    /// link itself). Returns `None` if the path can't be opened at all
    /// (e.g. permission denied) — callers treat that as "identity unknown."
    pub fn file_id_via_handle(path: &Path, follow: bool) -> Option<FileId> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let flags = FILE_FLAG_BACKUP_SEMANTICS | if follow { 0 } else { FILE_FLAG_OPEN_REPARSE_POINT };
        unsafe {
            let handle = CreateFileW(
                wide.as_ptr(),
                GENERIC_READ_NONE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                flags,
                std::ptr::null_mut(),
            );
            if handle as isize == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut info: ByHandleFileInformation = std::mem::zeroed();
            let ok = GetFileInformationByHandle(handle, &mut info);
            CloseHandle(handle);
            if ok == 0 {
                return None;
            }
            Some(FileId {
                volume_serial: info.volume_serial_number as u64,
                file_index: ((info.file_index_high as u64) << 32) | info.file_index_low as u64,
            })
        }
    }
}

#[cfg(windows)]
mod stream_enum {
    //! Minimal raw FFI over `FindFirstStreamW`/`FindNextStreamW` (available
    //! since Vista) to enumerate Alternate Data Streams. No external crate
    //! is pulled in for this — just the handful of kernel32 entry points
    //! needed.
    use std::ffi::{OsStr, OsString};
    use std::io;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::Path;

    #[repr(C)]
    struct Win32FindStreamData {
        stream_size: i64,
        stream_name: [u16; 296], // MAX_PATH(260) + 36, per WIN32_FIND_STREAM_DATA
    }

    #[allow(non_camel_case_types)]
    type HANDLE = *mut core::ffi::c_void;

    const INVALID_HANDLE_VALUE: isize = -1;
    const FIND_STREAM_INFO_STANDARD: u32 = 0;
    const ERROR_HANDLE_EOF: i32 = 38;
    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_NOT_SUPPORTED: i32 = 50;

    #[link(name = "kernel32")]
    extern "system" {
        fn FindFirstStreamW(lpFileName: *const u16, InfoLevel: u32, lpFindStreamData: *mut Win32FindStreamData, dwFlags: u32) -> HANDLE;
        fn FindNextStreamW(hFindStream: HANDLE, lpFindStreamData: *mut Win32FindStreamData) -> i32;
        fn FindClose(hFindFile: HANDLE) -> i32;
        fn GetLastError() -> u32;
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Stream names as returned by Windows, e.g. `":Zone.Identifier:$DATA"`.
    /// The unnamed default data stream (`::$DATA`) is filtered out — callers
    /// only care about *named* ADS.
    pub fn list_streams(path: &Path) -> io::Result<Vec<OsString>> {
        let wpath = wide(path);
        let mut data = Win32FindStreamData { stream_size: 0, stream_name: [0u16; 296] };
        let mut out = Vec::new();
        unsafe {
            let handle = FindFirstStreamW(wpath.as_ptr(), FIND_STREAM_INFO_STANDARD, &mut data, 0);
            if handle as isize == INVALID_HANDLE_VALUE {
                let err = GetLastError() as i32;
                return if err == ERROR_HANDLE_EOF || err == ERROR_FILE_NOT_FOUND || err == ERROR_NOT_SUPPORTED {
                    Ok(Vec::new())
                } else {
                    Err(io::Error::from_raw_os_error(err))
                };
            }
            loop {
                let name = decode_name(&data.stream_name);
                if name != ":$DATA" {
                    if let Some(stripped) = name.strip_suffix(":$DATA").and_then(|s| s.strip_prefix(':')) {
                        out.push(OsString::from(stripped));
                    }
                }
                if FindNextStreamW(handle, &mut data) == 0 {
                    let err = GetLastError() as i32;
                    if err != ERROR_HANDLE_EOF {
                        FindClose(handle);
                        return Err(io::Error::from_raw_os_error(err));
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

    /// Build the `path:stream` form used to address a named ADS.
    pub fn stream_path(path: &Path, stream: &OsStr) -> std::path::PathBuf {
        let mut s = path.as_os_str().to_os_string();
        s.push(":");
        s.push(stream);
        std::path::PathBuf::from(s)
    }
}

// ---------------------------------------------------------------------
// FakeFs — deterministic in-memory double for unit tests
// ---------------------------------------------------------------------

#[derive(Debug, Clone)]
enum FakeNode {
    File { size: u64, modified: Option<SystemTime>, readonly: bool, streams: BTreeMap<OsString, u64> },
    Dir { readonly: bool },
    Reparse { is_dir: bool, target: PathBuf, kind: ReparseKind },
}

/// Which fs-trait operation an [`Injection`] applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectableOp {
    Metadata,
    ReadDir,
    CreateDir,
    /// Fails a chunked copy after the given number of chunks have already
    /// succeeded (simulates disk-full mid-copy).
    CopyAfterChunks(u64),
    Rename,
    RemoveFile,
    RemoveDir,
    SetReadonly,
}

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
pub struct FakeFs {
    nodes: Mutex<BTreeMap<PathBuf, FakeNode>>,
    volumes: Mutex<BTreeMap<PathBuf, u64>>,
    ids: Mutex<BTreeMap<PathBuf, u64>>,
    next_id: AtomicU64,
    injections: Mutex<Vec<Injection>>,
}

impl Default for FakeFs {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeFs {
    pub fn new() -> Self {
        FakeFs {
            nodes: Mutex::new(BTreeMap::new()),
            volumes: Mutex::new(BTreeMap::new()),
            ids: Mutex::new(BTreeMap::new()),
            next_id: AtomicU64::new(1),
            injections: Mutex::new(Vec::new()),
        }
    }

    fn norm(path: &Path) -> PathBuf {
        super::path::lexically_normalize(path)
    }

    pub fn add_dir(&self, path: &Path) {
        self.nodes.lock().unwrap().insert(Self::norm(path), FakeNode::Dir { readonly: false });
    }

    pub fn add_file(&self, path: &Path, size: u64) {
        self.nodes
            .lock()
            .unwrap()
            .insert(Self::norm(path), FakeNode::File { size, modified: None, readonly: false, streams: BTreeMap::new() });
    }

    pub fn add_file_with_stream(&self, path: &Path, size: u64, stream_name: &str, stream_size: u64) {
        let mut streams = BTreeMap::new();
        streams.insert(OsString::from(stream_name), stream_size);
        self.nodes
            .lock()
            .unwrap()
            .insert(Self::norm(path), FakeNode::File { size, modified: None, readonly: false, streams });
    }

    pub fn add_reparse_point(&self, path: &Path, is_dir: bool, target: &Path, kind: ReparseKind) {
        self.nodes.lock().unwrap().insert(Self::norm(path), FakeNode::Reparse { is_dir, target: Self::norm(target), kind });
    }

    /// Assign an explicit volume tag to everything under `prefix`. Paths
    /// with no matching prefix default to volume `0`.
    pub fn set_volume(&self, prefix: &Path, volume: u64) {
        self.volumes.lock().unwrap().insert(Self::norm(prefix), volume);
    }

    fn volume_of(&self, path: &Path) -> u64 {
        let p = Self::norm(path);
        let volumes = self.volumes.lock().unwrap();
        let mut best: Option<(&PathBuf, &u64)> = None;
        for (prefix, vol) in volumes.iter() {
            if p.starts_with(prefix) {
                if best.map(|(b, _)| prefix.components().count() > b.components().count()).unwrap_or(true) {
                    best = Some((prefix, vol));
                }
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

    /// Resolve `path` the way real NTFS resolves a path through junctions:
    /// every *ancestor* component is transparently redirected if it's a
    /// reparse point, but the final component itself is left as-is (so
    /// callers can still see that the final component is a reparse point).
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
        let p = Self::norm(path);
        let mut nodes = self.nodes.lock().unwrap();
        if nodes.contains_key(&p) {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists));
        }
        nodes.insert(p, FakeNode::Dir { readonly: false });
        Ok(())
    }

    fn copy_file_chunked(
        &self,
        from: &Path,
        to: &Path,
        chunk_bytes: usize,
        on_chunk: &mut dyn FnMut(u64) -> ChunkControl,
    ) -> io::Result<u64> {
        let size = match self.metadata(from)? {
            FsMetadata { size, is_dir: false, .. } => size,
            _ => return Err(io::Error::from(io::ErrorKind::InvalidInput)),
        };
        let chunk = chunk_bytes.max(1) as u64;
        let mut done = 0u64;
        let mut chunk_index = 0u64;
        loop {
            let fail_after = {
                let mut injections = self.injections.lock().unwrap();
                let idx = injections
                    .iter()
                    .position(|i| matches!(i.op, InjectableOp::CopyAfterChunks(n) if n == chunk_index) && i.path == Self::norm(to));
                idx.map(|idx| injections.remove(idx).kind)
            };
            if let Some(kind) = fail_after {
                return Err(io::Error::from(kind));
            }
            let step = chunk.min(size - done);
            done += step;
            chunk_index += 1;
            if on_chunk(done) == ChunkControl::Cancel {
                return Err(cancelled_error());
            }
            if done >= size {
                break;
            }
        }
        self.nodes
            .lock()
            .unwrap()
            .insert(Self::norm(to), FakeNode::File { size, modified: None, readonly: false, streams: BTreeMap::new() });
        Ok(size)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        if let Some(kind) = self.take_injection(InjectableOp::Rename, from) {
            return Err(io::Error::from(kind));
        }
        let pf = Self::norm(from);
        let pt = Self::norm(to);
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
        let p = Self::norm(path);
        let mut nodes = self.nodes.lock().unwrap();
        match nodes.get(&p) {
            Some(FakeNode::File { readonly: true, .. }) => Err(io::Error::from(io::ErrorKind::PermissionDenied)),
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
        let p = Self::norm(path);
        let mut nodes = self.nodes.lock().unwrap();
        if nodes.keys().any(|k| k.parent() == Some(p.as_path())) {
            return Err(io::Error::new(io::ErrorKind::Other, "directory not empty"));
        }
        nodes.remove(&p).ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        Ok(())
    }

    fn set_readonly(&self, path: &Path, readonly: bool) -> io::Result<()> {
        if let Some(kind) = self.take_injection(InjectableOp::SetReadonly, path) {
            return Err(io::Error::from(kind));
        }
        let p = Self::norm(path);
        let mut nodes = self.nodes.lock().unwrap();
        match nodes.get_mut(&p) {
            Some(FakeNode::File { readonly: r, .. }) => *r = readonly,
            Some(FakeNode::Dir { readonly: r }) => *r = readonly,
            _ => return Err(io::Error::from(io::ErrorKind::NotFound)),
        }
        Ok(())
    }

    fn set_modified(&self, path: &Path, modified: SystemTime) -> io::Result<()> {
        let p = Self::norm(path);
        let mut nodes = self.nodes.lock().unwrap();
        match nodes.get_mut(&p) {
            Some(FakeNode::File { modified: m, .. }) => {
                *m = Some(modified);
                Ok(())
            }
            _ => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }

    fn list_streams(&self, path: &Path) -> io::Result<Vec<OsString>> {
        let p = Self::norm(path);
        let nodes = self.nodes.lock().unwrap();
        match nodes.get(&p) {
            Some(FakeNode::File { streams, .. }) => Ok(streams.keys().cloned().collect()),
            _ => Ok(Vec::new()),
        }
    }

    fn copy_stream_chunked(&self, from: &Path, stream: &OsStr, to: &Path, _chunk_bytes: usize) -> io::Result<u64> {
        let pf = Self::norm(from);
        let size = {
            let nodes = self.nodes.lock().unwrap();
            match nodes.get(&pf) {
                Some(FakeNode::File { streams, .. }) => *streams.get(stream).unwrap_or(&0),
                _ => 0,
            }
        };
        let pt = Self::norm(to);
        let mut nodes = self.nodes.lock().unwrap();
        if let Some(FakeNode::File { streams, .. }) = nodes.get_mut(&pt) {
            streams.insert(stream.to_os_string(), size);
        }
        Ok(size)
    }

    fn same_volume(&self, a: &Path, b: &Path) -> io::Result<bool> {
        Ok(self.volume_of(a) == self.volume_of(b))
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

    #[test]
    fn fake_fs_copy_disk_full_after_n_chunks() {
        let fake = FakeFs::new();
        fake.add_file(Path::new("/src.bin"), 30);
        fake.inject(InjectableOp::CopyAfterChunks(2), Path::new("/dst.bin"), io::ErrorKind::StorageFull);
        let mut chunks = 0u64;
        let err = fake
            .copy_file_chunked(Path::new("/src.bin"), Path::new("/dst.bin"), 10, &mut |_| {
                chunks += 1;
                ChunkControl::Continue
            })
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::StorageFull);
        assert_eq!(chunks, 2);
    }

    #[test]
    fn fake_fs_cancel_stops_copy_with_interrupted() {
        let fake = FakeFs::new();
        fake.add_file(Path::new("/src.bin"), 30);
        let err = fake
            .copy_file_chunked(Path::new("/src.bin"), Path::new("/dst.bin"), 10, &mut |done| {
                if done >= 10 {
                    ChunkControl::Cancel
                } else {
                    ChunkControl::Continue
                }
            })
            .unwrap_err();
        assert!(is_cancelled(&err));
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

    #[test]
    fn fake_fs_streams_roundtrip() {
        let fake = FakeFs::new();
        fake.add_file_with_stream(Path::new("/a.txt"), 5, "Zone.Identifier", 3);
        let streams = fake.list_streams(Path::new("/a.txt")).unwrap();
        assert_eq!(streams, vec![OsString::from("Zone.Identifier")]);
        fake.add_file(Path::new("/b.txt"), 5);
        fake.copy_stream_chunked(Path::new("/a.txt"), OsStr::new("Zone.Identifier"), Path::new("/b.txt"), 64).unwrap();
        assert_eq!(fake.list_streams(Path::new("/b.txt")).unwrap(), vec![OsString::from("Zone.Identifier")]);
    }
}
