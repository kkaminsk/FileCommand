//! Instant-open byte access for the F3 viewer.
//!
//! [`ByteSource::open`] keeps an open read handle on the file and serves
//! every read through bounded positioned reads on it, on every platform —
//! the file is never memory-mapped, so a concurrent writer can neither
//! crash the viewer (a truncated mapping raises SIGBUS on Unix) nor be
//! blocked by it (a mapping pins the file against truncation on Windows).
//! Open cost is O(1): only the handle and its length are touched, never the
//! content (viewer: Instant open — "First frame reads only the visible
//! window"). The length is captured once as an immutable snapshot and every
//! read is clamped to it, so navigation never reads past what was true at
//! open; a file that shrinks underneath simply reads short (viewer: Instant
//! open — "Offsets clamped to the snapshot length", "Truncation while
//! viewing never crashes").

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// A byte-addressable, read-only view onto a file.
pub struct ByteSource {
    len: u64,
    file: File,
}

impl ByteSource {
    /// Open `path` for positioned reads. On Windows the handle is opened
    /// with read, write, and delete sharing, so other programs can keep
    /// appending to, truncating, deleting, or renaming the file while it is
    /// shown (viewer: Instant open — "Viewed file can be truncated,
    /// rotated, or deleted").
    pub fn open(path: &Path) -> io::Result<ByteSource> {
        let file = open_shared(path)?;
        let len = file.metadata()?.len();
        Ok(ByteSource { len, file })
    }

    /// The length captured at open. Immutable for the lifetime of this
    /// `ByteSource`, regardless of subsequent changes to the on-disk file.
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Read up to `len` bytes starting at `offset`, clamped to the snapshot
    /// length so a read never crosses what was true at open. Returns fewer
    /// than `len` bytes (down to zero) once the window runs past the end —
    /// including when the file has shrunk since open, where the short read
    /// is simply the new end of file. An I/O error maps to an empty window,
    /// never a panic.
    pub fn read_range(&self, offset: u64, len: usize) -> Vec<u8> {
        if offset >= self.len {
            return Vec::new();
        }
        let end = offset.saturating_add(len as u64).min(self.len);
        let n = (end - offset) as usize;
        let mut buf = vec![0u8; n];
        let read = positioned_read_to_eof(&self.file, &mut buf, offset).unwrap_or(0);
        buf.truncate(read);
        buf
    }
}

#[cfg(windows)]
fn open_shared(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    // The spec's sharing guarantee, stated explicitly rather than left to
    // the standard library's default (which happens to be the same today):
    // other programs can keep writing to, truncating, deleting, and
    // renaming the viewed file (viewer: Instant open — "The viewer MUST NOT
    // prevent other programs from changing the file while it is shown").
    const FILE_SHARE_READ: u32 = 0x1;
    const FILE_SHARE_WRITE: u32 = 0x2;
    const FILE_SHARE_DELETE: u32 = 0x4;
    OpenOptions::new().read(true).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).open(path)
}

#[cfg(not(windows))]
fn open_shared(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).open(path)
}

#[cfg(windows)]
fn positioned_read(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_read(buf, offset)
}

#[cfg(unix)]
fn positioned_read(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.read_at(buf, offset)
}

#[cfg(not(any(windows, unix)))]
fn positioned_read(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::io::{Read, Seek, SeekFrom};
    // A `ByteSource` is used by one thread at a time — the render loop owns
    // the viewer's source, and the F7 search worker opens its own — so the
    // shared cursor this seek moves cannot race. A future change that
    // shares one source across threads must revisit this fallback.
    let mut handle = file;
    handle.seek(SeekFrom::Start(offset))?;
    handle.read(buf)
}

/// Fill `buf` from `file` starting at `offset`, looping past short reads
/// (`seek_read`/`read_at` may return fewer bytes than requested) until `buf`
/// is full or EOF is reached.
fn positioned_read_to_eof(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    let mut total = 0usize;
    while total < buf.len() {
        match positioned_read(file, &mut buf[total..], offset + total as u64) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(name: &str, contents: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("filecommand-viewer-test-{}-{}", std::process::id(), name));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("file.bin");
        let mut f = File::create(&path).unwrap();
        f.write_all(contents).unwrap();
        f.flush().unwrap();
        path
    }

    #[test]
    fn open_reports_correct_length() {
        let path = temp_file("length", b"hello world");
        let src = ByteSource::open(&path).unwrap();
        assert_eq!(src.len(), 11);
        assert!(!src.is_empty());
    }

    #[test]
    fn open_empty_file_reports_zero_length() {
        let path = temp_file("empty", b"");
        let src = ByteSource::open(&path).unwrap();
        assert_eq!(src.len(), 0);
        assert!(src.is_empty());
        assert_eq!(src.read_range(0, 10), Vec::<u8>::new());
    }

    #[test]
    fn read_range_returns_requested_window() {
        let path = temp_file("window", b"0123456789");
        let src = ByteSource::open(&path).unwrap();
        assert_eq!(src.read_range(2, 3), b"234".to_vec());
        assert_eq!(src.read_range(0, 100), b"0123456789".to_vec());
    }

    #[test]
    fn read_range_clamps_to_snapshot_length() {
        let path = temp_file("clamp", b"0123456789");
        let src = ByteSource::open(&path).unwrap();
        assert_eq!(src.read_range(8, 10), b"89".to_vec());
        assert_eq!(src.read_range(20, 10), Vec::<u8>::new());
        assert_eq!(src.read_range(10, 5), Vec::<u8>::new());
    }

    #[test]
    fn open_missing_path_is_an_error_not_a_panic() {
        let missing = std::env::temp_dir().join("filecommand-viewer-test-does-not-exist.bin");
        assert!(ByteSource::open(&missing).is_err());
    }

    /// viewer: Instant open — "Another program keeps writing to the viewed
    /// file": the append succeeds while the file is shown, and reads stay
    /// clamped to the length captured at open.
    #[test]
    fn appending_through_a_second_handle_succeeds_and_reads_stay_clamped_to_the_snapshot() {
        let path = temp_file("append", b"0123456789");
        let src = ByteSource::open(&path).unwrap();
        let mut writer = OpenOptions::new().append(true).open(&path).unwrap();
        writer.write_all(b"-appended").unwrap();
        writer.flush().unwrap();
        assert_eq!(src.len(), 10, "the snapshot length does not move when the file grows");
        assert_eq!(src.read_range(0, 100), b"0123456789".to_vec());
        assert_eq!(src.read_range(10, 5), Vec::<u8>::new());
    }

    /// viewer: Instant open — "Truncation while viewing never crashes",
    /// "Viewed file can be truncated, rotated, or deleted": truncating the
    /// open file from a second handle succeeds (with the old mmap backend
    /// this failed on Windows with OS error 1224, ERROR_USER_MAPPED_FILE;
    /// on Unix the next read past the new end raised SIGBUS), and reads
    /// past the new end come back short or empty without panicking.
    #[test]
    fn truncating_while_open_succeeds_and_reads_past_the_new_end_come_back_short() {
        let contents = vec![b'x'; 10 * 1024];
        let path = temp_file("truncate", &contents);
        let src = ByteSource::open(&path).unwrap();
        let writer = OpenOptions::new().write(true).open(&path).unwrap();
        writer.set_len(1024).unwrap();
        drop(writer);
        assert_eq!(src.read_range(5000, 100), Vec::<u8>::new());
        assert_eq!(src.read_range(0, 2000).len(), 1024);
        assert_eq!(src.len(), 10 * 1024, "the snapshot length still clamps navigation");
    }

    /// viewer: Instant open — "Viewed file can be truncated, rotated, or
    /// deleted": rename and delete both succeed while the source is open
    /// (both need the delete share on Windows), and the open handle keeps
    /// serving the bytes it opened.
    #[test]
    fn renaming_and_deleting_while_open_succeed() {
        let path = temp_file("rename-delete", b"hello world");
        let src = ByteSource::open(&path).unwrap();
        let renamed = path.with_file_name("renamed.bin");
        std::fs::rename(&path, &renamed).unwrap();
        assert_eq!(src.read_range(0, 5), b"hello".to_vec());
        std::fs::remove_file(&renamed).unwrap();
        assert_eq!(src.read_range(0, 5), b"hello".to_vec(), "the open handle keeps the bytes readable");
        drop(src);
        let _ = std::fs::remove_dir(path.parent().unwrap());
    }
}
