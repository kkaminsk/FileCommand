//! Directory reading: the fs-access seam, long-path handling, entry data
//! model, and streaming enumeration designed to be driven from a worker
//! thread (owned by `filecommand-tui`, never spawned here).

use std::cmp::Ordering;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use unicode_width::UnicodeWidthStr;

/// What kind of thing an [`Entry`] refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    /// The synthetic `..` parent-directory entry.
    ParentDir,
}

/// A simple, dependency-free civil calendar timestamp (UTC), used for the
/// panel's date/time columns without pulling in a chrono-style dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DateTime {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
}

impl DateTime {
    /// Convert seconds since the Unix epoch (UTC) into a civil date/time,
    /// using Howard Hinnant's `civil_from_days` algorithm.
    pub fn from_unix_seconds(total_secs: i64) -> DateTime {
        let days = total_secs.div_euclid(86_400);
        let secs_of_day = total_secs.rem_euclid(86_400);
        let hour = (secs_of_day / 3600) as u8;
        let minute = ((secs_of_day % 3600) / 60) as u8;

        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = (z - era * 146_097) as i64; // [0, 146096]
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
        let mp = (5 * doy + 2) / 153; // [0, 11]
        let day = (doy - (153 * mp + 2) / 5 + 1) as u8; // [1, 31]
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8; // [1, 12]
        let year = if month <= 2 { y + 1 } else { y };

        DateTime { year: year as i32, month, day, hour, minute }
    }

    pub fn from_system_time(t: SystemTime) -> DateTime {
        let secs = match t.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => d.as_secs() as i64,
            Err(e) => -(e.duration().as_secs() as i64),
        };
        DateTime::from_unix_seconds(secs)
    }
}

/// One directory entry as displayed in a panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: OsString,
    pub kind: EntryKind,
    pub size: u64,
    pub modified: Option<DateTime>,
}

impl Entry {
    pub fn parent_dir() -> Entry {
        Entry { name: OsString::from(".."), kind: EntryKind::ParentDir, size: 0, modified: None }
    }

    pub fn is_dir_like(&self) -> bool {
        matches!(self.kind, EntryKind::Directory | EntryKind::ParentDir)
    }
}

/// Lossy UTF-8 rendering of a possibly-non-Unicode file name.
pub fn display_name_lossy(entry: &Entry) -> String {
    entry.name.to_string_lossy().into_owned()
}

// ---------------------------------------------------------------------
// Sort modes and comparators
// ---------------------------------------------------------------------

/// Which key a panel is sorted by. Set per panel with Ctrl+F3..Ctrl+F7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortMode {
    #[default]
    Name,
    Extension,
    Time,
    Size,
    /// Directory-enumeration order: no reordering at all beyond floating
    /// `..` to the top.
    Unsorted,
}

/// The panel column a sort mode marks with its `↓`/`↑` arrow. `Unsorted`
/// marks no column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Size,
    Date,
}

impl SortMode {
    /// The header column this mode's arrow indicator belongs on, or `None`
    /// in `Unsorted` mode where no column carries an arrow.
    pub fn column(self) -> Option<SortColumn> {
        match self {
            // Extension sorting is still a name-column ordering in NC — the
            // arrow stays on Name.
            SortMode::Name | SortMode::Extension => Some(SortColumn::Name),
            SortMode::Size => Some(SortColumn::Size),
            // One modification timestamp is displayed across the Date and
            // Time columns; the arrow marks the first of the pair.
            SortMode::Time => Some(SortColumn::Date),
            SortMode::Unsorted => None,
        }
    }
}

/// The `↓`/`↑` indicator for a sort direction. Both are CP437-heritage
/// glyphs.
pub fn sort_arrow(descending: bool) -> &'static str {
    if descending {
        "\u{2191}"
    } else {
        "\u{2193}"
    }
}

fn lower_name(entry: &Entry) -> String {
    entry.name.to_string_lossy().to_lowercase()
}

/// The DOS-style extension of a name: everything after the last `.`, except
/// that a leading dot is part of the name (`.gitignore` has no extension).
pub fn extension_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(0) | None => "",
        Some(i) => &name[i + 1..],
    }
}

/// Case-insensitive name order. Directories and files interleave, matching
/// classic Norton Commander behavior.
pub fn cmp_by_name(a: &Entry, b: &Entry) -> Ordering {
    lower_name(a).cmp(&lower_name(b))
}

/// Extension order, falling back to name so entries sharing an extension
/// stay in a predictable (total) order.
pub fn cmp_by_extension(a: &Entry, b: &Entry) -> Ordering {
    let (an, bn) = (lower_name(a), lower_name(b));
    extension_of(&an).cmp(extension_of(&bn)).then_with(|| an.cmp(&bn))
}

/// Modification-time order, oldest first; entries with no timestamp sort
/// before any timestamped entry. Ties break on name.
pub fn cmp_by_time(a: &Entry, b: &Entry) -> Ordering {
    a.modified.cmp(&b.modified).then_with(|| lower_name(a).cmp(&lower_name(b)))
}

/// Size order, smallest first, ties breaking on name. Directories carry
/// size 0 and therefore group at the front.
pub fn cmp_by_size(a: &Entry, b: &Entry) -> Ordering {
    a.size.cmp(&b.size).then_with(|| lower_name(a).cmp(&lower_name(b)))
}

/// The comparator for `mode`, before `..`-first and direction handling.
/// `Unsorted` compares every pair equal, so a *stable* sort leaves
/// enumeration order untouched.
pub fn cmp_by_mode(a: &Entry, b: &Entry, mode: SortMode) -> Ordering {
    match mode {
        SortMode::Name => cmp_by_name(a, b),
        SortMode::Extension => cmp_by_extension(a, b),
        SortMode::Time => cmp_by_time(a, b),
        SortMode::Size => cmp_by_size(a, b),
        SortMode::Unsorted => Ordering::Equal,
    }
}

#[cfg(test)]
mod sort_tests {
    use super::*;

    fn f(name: &str, size: u64, modified: Option<DateTime>) -> Entry {
        Entry { name: OsString::from(name), kind: EntryKind::File, size, modified }
    }

    fn dt(day: u8) -> DateTime {
        DateTime { year: 2026, month: 1, day, hour: 0, minute: 0 }
    }

    #[test]
    fn extension_of_ignores_a_leading_dot() {
        assert_eq!(extension_of("readme.txt"), "txt");
        assert_eq!(extension_of("archive.tar.gz"), "gz");
        assert_eq!(extension_of(".gitignore"), "");
        assert_eq!(extension_of("noext"), "");
    }

    #[test]
    fn each_comparator_orders_by_its_own_key() {
        assert_eq!(cmp_by_name(&f("apple", 9, None), &f("Banana", 1, None)), Ordering::Less);
        assert_eq!(cmp_by_extension(&f("z.aaa", 0, None), &f("a.zzz", 0, None)), Ordering::Less);
        assert_eq!(cmp_by_size(&f("big", 100, None), &f("small", 1, None)), Ordering::Greater);
        assert_eq!(cmp_by_time(&f("old", 0, Some(dt(1))), &f("new", 0, Some(dt(2)))), Ordering::Less);
    }

    #[test]
    fn unsorted_compares_everything_equal() {
        assert_eq!(cmp_by_mode(&f("z", 9, None), &f("a", 1, None), SortMode::Unsorted), Ordering::Equal);
    }

    #[test]
    fn sort_column_maps_modes_and_unsorted_has_none() {
        assert_eq!(SortMode::Name.column(), Some(SortColumn::Name));
        assert_eq!(SortMode::Extension.column(), Some(SortColumn::Name));
        assert_eq!(SortMode::Size.column(), Some(SortColumn::Size));
        assert_eq!(SortMode::Time.column(), Some(SortColumn::Date));
        assert_eq!(SortMode::Unsorted.column(), None);
    }

    #[test]
    fn sort_arrow_points_down_for_ascending() {
        assert_eq!(sort_arrow(false), "\u{2193}");
        assert_eq!(sort_arrow(true), "\u{2191}");
    }
}

/// Display (column) width of a string, accounting for wide/combining
/// characters via `unicode-width`.
pub fn display_width(s: &str) -> usize {
    s.width()
}

/// Right-pad / truncate `s` to exactly `width` display columns.
pub fn pad_to_width(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        let mut out = String::new();
        let mut acc = 0usize;
        for ch in s.chars() {
            let cw = UnicodeWidthStr::width(ch.to_string().as_str());
            if acc + cw > width {
                break;
            }
            out.push(ch);
            acc += cw;
        }
        while display_width(&out) < width {
            out.push(' ');
        }
        out
    } else {
        let mut out = s.to_string();
        out.push_str(&" ".repeat(width - w));
        out
    }
}

/// Human-readable byte size, NC-style (whole numbers, then K/M/G suffix).
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["", "K", "M", "G"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes}")
    } else {
        format!("{value:.0}{}", UNITS[unit])
    }
}

/// `MM-DD-YY`
pub fn format_date(dt: DateTime) -> String {
    format!("{:02}-{:02}-{:02}", dt.month, dt.day, dt.year.rem_euclid(100))
}

/// `HH:MM`
pub fn format_time(dt: DateTime) -> String {
    format!("{:02}:{:02}", dt.hour, dt.minute)
}

/// The mini-status text for a completed listing's currently-selected entry:
/// `name  size  date  time`.
pub fn entry_status_line(entry: &Entry) -> String {
    let name = display_name_lossy(entry);
    match entry.kind {
        EntryKind::ParentDir => format!("{name}  UP--DIR"),
        EntryKind::Directory => format!("{name}  SUB-DIR"),
        EntryKind::File => {
            let dt = entry.modified.map(|d| format!("{} {}", format_date(d), format_time(d))).unwrap_or_default();
            format!("{name}  {}  {dt}", format_size(entry.size))
        }
    }
}

/// Comma-grouped integer, e.g. `12_345 -> "12,345"`.
pub fn format_count(n: usize) -> String {
    let digits: Vec<u8> = n.to_string().into_bytes();
    let mut out = Vec::with_capacity(digits.len() + digits.len() / 3);
    for (i, d) in digits.iter().rev().enumerate() {
        if i != 0 && i % 3 == 0 {
            out.push(b',');
        }
        out.push(*d);
    }
    out.reverse();
    String::from_utf8(out).expect("digits and commas are valid utf8")
}

/// The mini-status text shown while a listing is still streaming.
pub fn reading_status(count: usize) -> String {
    format!("Reading\u{2026} {}", format_count(count))
}

// ---------------------------------------------------------------------
// Long-path handling
// ---------------------------------------------------------------------

/// Apply the `\\?\` long-path prefix to an absolute Windows path so listing
/// reads are not subject to MAX_PATH. A no-op on non-Windows targets and for
/// paths that are already prefixed or not absolute.
#[cfg(windows)]
pub fn to_long_path(path: &Path) -> PathBuf {
    let s = path.as_os_str().to_string_lossy();
    if s.starts_with(r"\\?\") {
        return path.to_path_buf();
    }
    if let Some(rest) = s.strip_prefix(r"\\") {
        return PathBuf::from(format!(r"\\?\UNC\{rest}"));
    }
    if path.is_absolute() {
        return PathBuf::from(format!(r"\\?\{s}"));
    }
    path.to_path_buf()
}

#[cfg(not(windows))]
pub fn to_long_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

// ---------------------------------------------------------------------
// fs-access seam
// ---------------------------------------------------------------------

/// A single raw directory-enumeration record, as read from the filesystem
/// before it is turned into an [`Entry`]. Metadata here comes from the
/// enumeration itself (e.g. `FindNextFile` / `readdir`), not a per-file
/// `stat` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDirEntry {
    pub name: OsString,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// A narrow seam over directory reads. Exists so later milestones can inject
/// deterministic errors (permission denied, I/O failure, long-path edge
/// cases) without touching the real filesystem.
pub trait FsReader {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<RawDirEntry>>;
}

/// The real filesystem, via `std::fs`, routed through [`to_long_path`].
#[derive(Debug, Default, Clone, Copy)]
pub struct StdFsReader;

impl FsReader for StdFsReader {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<RawDirEntry>> {
        let long = to_long_path(path);
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&long)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            out.push(RawDirEntry {
                name: entry.file_name(),
                is_dir: metadata.is_dir(),
                size: if metadata.is_dir() { 0 } else { metadata.len() },
                modified: metadata.modified().ok(),
            });
        }
        Ok(out)
    }
}

impl From<RawDirEntry> for Entry {
    fn from(raw: RawDirEntry) -> Self {
        Entry {
            name: raw.name,
            kind: if raw.is_dir { EntryKind::Directory } else { EntryKind::File },
            size: raw.size,
            modified: raw.modified.map(DateTime::from_system_time),
        }
    }
}

/// Read a directory and deliver entries in chunks via `on_chunk`, followed
/// by a final `true` on completion. Designed to be called from a worker
/// thread by `filecommand-tui`; core never spawns threads itself.
pub fn list_dir_chunked<F: FnMut(Vec<Entry>)>(
    reader: &dyn FsReader,
    path: &Path,
    chunk_size: usize,
    mut on_chunk: F,
) -> io::Result<usize> {
    let raw = reader.read_dir(path)?;
    let mut total = 0usize;
    for chunk in raw.chunks(chunk_size.max(1)) {
        let entries: Vec<Entry> = chunk.iter().cloned().map(Entry::from).collect();
        total += entries.len();
        on_chunk(entries);
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_count_groups_by_thousands() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(5), "5");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_000), "1,000");
        assert_eq!(format_count(12_345), "12,345");
        assert_eq!(format_count(1_234_567), "1,234,567");
    }

    #[test]
    fn reading_status_formats_with_ellipsis_and_grouping() {
        assert_eq!(reading_status(12_345), "Reading\u{2026} 12,345");
    }

    #[test]
    fn date_time_from_unix_seconds_known_value() {
        // 2026-08-07 14:36 UTC
        let dt = DateTime::from_unix_seconds(1_786_113_360);
        assert_eq!(dt, DateTime { year: 2026, month: 8, day: 7, hour: 14, minute: 36 });
    }

    #[test]
    fn date_time_epoch_is_1970_01_01() {
        let dt = DateTime::from_unix_seconds(0);
        assert_eq!(dt, DateTime { year: 1970, month: 1, day: 1, hour: 0, minute: 0 });
    }

    #[test]
    fn format_date_and_time_are_two_digit_padded() {
        let dt = DateTime { year: 2026, month: 1, day: 2, hour: 3, minute: 4 };
        assert_eq!(format_date(dt), "01-02-26");
        assert_eq!(format_time(dt), "03:04");
    }

    #[test]
    fn non_unicode_name_displays_lossy_and_widths_are_stable() {
        #[cfg(unix)]
        {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt;
            let raw = OsStr::from_bytes(&[0x66, 0x6f, 0x80, 0x6f]); // "fo\x80o", invalid utf8
            let entry = Entry { name: raw.to_os_string(), kind: EntryKind::File, size: 0, modified: None };
            let shown = display_name_lossy(&entry);
            assert!(shown.contains('\u{FFFD}'));
            assert_eq!(display_width(&shown), shown.chars().count());
        }
        // On Windows, OsString cannot easily hold ill-formed UTF-16 in a test
        // without unsafe WTF-8 construction; the lossy path is exercised via
        // the Unix branch and by construction (to_string_lossy is always
        // total), so this test focuses coverage where it's reachable safely.
    }

    #[test]
    fn pad_to_width_pads_and_truncates() {
        assert_eq!(pad_to_width("abc", 5), "abc  ");
        assert_eq!(pad_to_width("abcdef", 3), "abc");
        assert_eq!(display_width(&pad_to_width("abc", 5)), 5);
    }

    #[test]
    fn entry_status_line_marks_dirs_and_parent() {
        let dir = Entry { name: "sub".into(), kind: EntryKind::Directory, size: 0, modified: None };
        assert_eq!(entry_status_line(&dir), "sub  SUB-DIR");
        assert_eq!(entry_status_line(&Entry::parent_dir()), "..  UP--DIR");
    }

    #[cfg(windows)]
    #[test]
    fn to_long_path_prefixes_absolute_windows_paths() {
        let p = to_long_path(Path::new(r"C:\Users\test"));
        assert_eq!(p, PathBuf::from(r"\\?\C:\Users\test"));
        // idempotent
        assert_eq!(to_long_path(&p), p);
    }

    #[cfg(windows)]
    #[test]
    fn to_long_path_handles_unc() {
        let p = to_long_path(Path::new(r"\\server\share\file"));
        assert_eq!(p, PathBuf::from(r"\\?\UNC\server\share\file"));
    }

    #[test]
    fn to_long_path_leaves_relative_paths_alone() {
        let p = to_long_path(Path::new("relative/path"));
        assert_eq!(p, PathBuf::from("relative/path"));
    }

    struct FakeReader(Vec<RawDirEntry>);
    impl FsReader for FakeReader {
        fn read_dir(&self, _path: &Path) -> io::Result<Vec<RawDirEntry>> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn list_dir_chunked_delivers_chunks_and_total() {
        let entries: Vec<RawDirEntry> = (0..10)
            .map(|i| RawDirEntry { name: format!("f{i}").into(), is_dir: false, size: 0, modified: None })
            .collect();
        let reader = FakeReader(entries);
        let mut chunks = Vec::new();
        let total = list_dir_chunked(&reader, Path::new("."), 3, |c| chunks.push(c.len())).unwrap();
        assert_eq!(total, 10);
        assert_eq!(chunks, vec![3, 3, 3, 1]);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn to_long_path_is_idempotent(segment in "[a-zA-Z0-9_]{1,8}") {
                #[cfg(windows)]
                let p = PathBuf::from(format!(r"C:\{segment}\sub"));
                #[cfg(not(windows))]
                let p = PathBuf::from(format!("/{segment}/sub"));
                let once = to_long_path(&p);
                let twice = to_long_path(&once);
                prop_assert_eq!(once, twice);
            }

            #[test]
            fn to_long_path_joined_child_stays_under_prefixed_parent(parent in "[a-zA-Z0-9_]{1,8}", child in "[a-zA-Z0-9_]{1,8}") {
                #[cfg(windows)]
                {
                    let base = PathBuf::from(format!(r"C:\{parent}"));
                    let joined = base.join(&child);
                    let long = to_long_path(&joined);
                    let shown = long.to_string_lossy();
                    prop_assert!(shown.starts_with(r"\\?\"));
                    let suffix = format!(r"{parent}\{child}");
                    prop_assert!(shown.ends_with(&suffix));
                }
                #[cfg(not(windows))]
                {
                    let base = PathBuf::from(format!("/{parent}"));
                    let joined = base.join(&child);
                    prop_assert_eq!(to_long_path(&joined), joined);
                }
            }

            #[test]
            fn format_count_roundtrips_digits(n in 0usize..10_000_000) {
                let grouped = format_count(n);
                let digits_only: String = grouped.chars().filter(|c| *c != ',').collect();
                prop_assert_eq!(digits_only, n.to_string());
            }
        }
    }
}
