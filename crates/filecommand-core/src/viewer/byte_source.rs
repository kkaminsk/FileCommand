//! Instant-open byte access for the F3 viewer.
//!
//! [`ByteSource::open`] memory-maps the file, falling back to positioned
//! chunk reads when mapping is unavailable (a network path, a zero-length
//! special file, ...). Either way, open cost is O(1): only the file handle
//! and its length are touched, never the content (viewer: Instant open —
//! "First frame reads only the visible window"). The length is captured
//! once as an immutable snapshot and every read is clamped to it, so
//! navigation never reads past what was true at open even if the on-disk
//! file changes underneath us (viewer: Instant open — "Offsets clamped to
//! the snapshot length").

use std::fs::File;
use std::io;
use std::path::Path;

enum Backend {
    Mmap(memmap2::Mmap),
    File(File),
}

/// A byte-addressable, read-only view onto a file.
pub struct ByteSource {
    len: u64,
    backend: Backend,
}

impl ByteSource {
    /// Open `path`, memory-mapping it when possible. `Mmap::map` fails on a
    /// zero-length file (nothing to map) and on some network/special paths;
    /// either way this falls back to positioned reads on the open handle
    /// rather than surfacing an error (viewer: Instant open — "Mapping
    /// unavailable falls back to chunk reads").
    pub fn open(path: &Path) -> io::Result<ByteSource> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        let backend = match unsafe { memmap2::Mmap::map(&file) } {
            Ok(mmap) => Backend::Mmap(mmap),
            Err(_) => Backend::File(file),
        };
        Ok(ByteSource { len, backend })
    }

    /// Open `path` via the chunk-read fallback unconditionally, bypassing
    /// mmap even when it would succeed. Exists so tests can compare the
    /// fallback path's bytes against [`ByteSource::open`]'s on the same file
    /// (viewer: Instant open — "Mapping unavailable falls back to chunk
    /// reads": "the rendered content is identical to the mmap path for the
    /// same byte range").
    pub fn open_chunked(path: &Path) -> io::Result<ByteSource> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(ByteSource { len, backend: Backend::File(file) })
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
    /// than `len` bytes (down to zero) once the window runs past the end.
    pub fn read_range(&self, offset: u64, len: usize) -> Vec<u8> {
        if offset >= self.len {
            return Vec::new();
        }
        let end = offset.saturating_add(len as u64).min(self.len);
        let n = (end - offset) as usize;
        match &self.backend {
            Backend::Mmap(mmap) => mmap[offset as usize..offset as usize + n].to_vec(),
            Backend::File(file) => {
                let mut buf = vec![0u8; n];
                let read = positioned_read_to_eof(file, &mut buf, offset).unwrap_or(0);
                buf.truncate(read);
                buf
            }
        }
    }
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
fn positioned_read(_file: &File, _buf: &mut [u8], _offset: u64) -> io::Result<usize> {
    Ok(0)
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
    fn open_empty_file_falls_back_and_reports_zero_length() {
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
    fn chunked_fallback_serves_identical_bytes_to_the_mmap_path() {
        let contents: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let path = temp_file("fallback-parity", &contents);
        let mmapped = ByteSource::open(&path).unwrap();
        let chunked = ByteSource::open_chunked(&path).unwrap();
        assert_eq!(mmapped.len(), chunked.len());
        for (offset, len) in [(0u64, 100usize), (1234, 500), (4900, 200), (0, 5000)] {
            assert_eq!(mmapped.read_range(offset, len), chunked.read_range(offset, len), "offset={offset} len={len}");
        }
    }

    #[test]
    fn chunked_open_offsets_are_also_clamped_to_snapshot_length() {
        let path = temp_file("fallback-clamp", b"0123456789");
        let src = ByteSource::open_chunked(&path).unwrap();
        assert_eq!(src.read_range(8, 10), b"89".to_vec());
        assert_eq!(src.read_range(20, 10), Vec::<u8>::new());
    }

    #[test]
    fn open_missing_path_is_an_error_not_a_panic() {
        let missing = std::env::temp_dir().join("filecommand-viewer-test-does-not-exist.bin");
        assert!(ByteSource::open(&missing).is_err());
    }
}
