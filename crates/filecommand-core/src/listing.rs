//! Directory reading: the fs-access seam, long-path handling, entry data
//! model, and streaming enumeration designed to be driven from a worker
//! thread (owned by `filecommand-tui`, never spawned here).

use std::cmp::Ordering;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use unicode_width::UnicodeWidthStr;

use crate::fs_ops::CancelFlag;

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
    /// The entry carries the Windows hidden/system attribute, or — on other
    /// platforms — a dot-prefixed name (hidden-entry-styling; design D1).
    /// The renderer dims such entries via the `panel.hidden` role. The
    /// synthetic `..` entry is never hidden.
    pub hidden: bool,
    /// The entry is a link — a junction or symbolic link on Windows, a
    /// symbolic link elsewhere — classified by following it once: `kind`
    /// and `size` describe the target, while `name`, `modified` and
    /// `hidden` are the link's own (panel-navigation "Directory links list
    /// as directories"; link-aware-listing design D1/D3). Internal only —
    /// nothing renders it. Find-file never descends into a directory link,
    /// and F8 on one asks a single confirmation because Delete removes only
    /// the link (design D4/D5). The synthetic `..` entry is never a link.
    pub link: bool,
}

impl Entry {
    pub fn parent_dir() -> Entry {
        Entry { name: OsString::from(".."), kind: EntryKind::ParentDir, size: 0, modified: None, hidden: false, link: false }
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
        Entry { name: OsString::from(name), kind: EntryKind::File, size, modified, hidden: false, link: false }
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

/// The same fields `entry_status_line` joins into one string, kept apart so
/// a narrow mini-status can drop them individually — time, then date, then
/// size — in the same rightmost-first order as the Full-mode column ladder
/// (responsive-layout "Chrome degradation"). `..`/directories carry their
/// `UP--DIR`/`SUB-DIR` marker in the size slot and never have a date or
/// time.
pub fn entry_status_parts(entry: &Entry) -> (String, Option<String>, Option<String>, Option<String>) {
    let name = display_name_lossy(entry);
    match entry.kind {
        EntryKind::ParentDir => (name, Some("UP--DIR".to_string()), None, None),
        EntryKind::Directory => (name, Some("SUB-DIR".to_string()), None, None),
        EntryKind::File => {
            let size = Some(format_size(entry.size));
            let date = entry.modified.map(format_date);
            let time = entry.modified.map(format_time);
            (name, size, date, time)
        }
    }
}

/// Truncate `s` to at most `max_w` display columns, appending `…` only when
/// truncation actually occurred (a short-enough `s` is returned unchanged).
/// Display-width aware (`unicode-width`), used to clamp overlay interiors
/// and mini-status/command-line text that no longer fits its computed
/// width (responsive-layout "Unified overlay geometry", "Chrome
/// degradation").
pub fn truncate_with_ellipsis(s: &str, max_w: usize) -> String {
    if display_width(s) <= max_w {
        return s.to_string();
    }
    if max_w == 0 {
        return String::new();
    }
    if max_w == 1 {
        return "\u{2026}".to_string();
    }
    let budget = max_w - 1; // reserve one cell for the ellipsis
    let mut out = String::new();
    let mut acc = 0usize;
    for ch in s.chars() {
        let cw = display_width(&ch.to_string());
        if acc + cw > budget {
            break;
        }
        out.push(ch);
        acc += cw;
    }
    out.push('\u{2026}');
    out
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
    /// The hidden/system flag, computed once by the reader (hidden-entry-
    /// styling; design D1): on Windows from the `FILE_ATTRIBUTE_HIDDEN`
    /// (0x2) / `FILE_ATTRIBUTE_SYSTEM` (0x4) bits in the enumeration's own
    /// metadata, elsewhere from a leading `.` in the name.
    pub hidden: bool,
    /// The entry is a link (see [`Entry::link`]). When set, `is_dir` and
    /// `size` describe the link's target — or `false`/`0` for a link that
    /// could not be followed — while `name`, `modified` and `hidden` are the
    /// link's own (link-aware-listing design D1/D2).
    pub link: bool,
}

/// One directory read: the entries that could be classified, plus how many
/// were skipped because their own metadata could not be read (panel-
/// navigation "One unreadable entry does not fail a listing"; link-aware-
/// listing design D6). Skipping is per entry; a failure of the enumeration
/// itself is still the `Err` of [`FsReader::read_dir`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawDirListing {
    pub entries: Vec<RawDirEntry>,
    pub unreadable: usize,
}

/// A narrow seam over directory reads. Exists so later milestones can inject
/// deterministic errors (permission denied, I/O failure, long-path edge
/// cases) without touching the real filesystem.
pub trait FsReader {
    fn read_dir(&self, path: &Path) -> io::Result<RawDirListing>;
}

/// What a reader learns about one enumerated entry from a single metadata
/// record — the enumeration's own (`DirEntry::metadata`, which never follows
/// a link) or the target's (`std::fs::metadata`, which does). The pure input
/// of [`classify_entry`], so every classification branch is testable without
/// a filesystem (link-aware-listing design D8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryFacts {
    pub is_dir: bool,
    pub is_symlink: bool,
    pub len: u64,
    pub modified: Option<SystemTime>,
    pub hidden: bool,
}

/// Classify one enumerated entry (link-aware-listing design D1/D2/D8):
///
/// - `own` is `Err` → `None`: the entry's own metadata could not be read, so
///   it is skipped and the caller counts it as unreadable (design D6).
/// - not a link → exactly the entry's own facts, no extra I/O.
/// - a link → `follow()` is called once. On `Ok`, `is_dir` and (for a file
///   target) `size` come from the target; on `Err` — dangling, or a target
///   that cannot be reached — the entry lists as a non-enterable file of
///   size 0. Either way `modified` and `hidden` stay the link's own, and
///   `link` is set.
pub fn classify_entry(name: OsString, own: io::Result<EntryFacts>, follow: impl FnOnce() -> io::Result<EntryFacts>) -> Option<RawDirEntry> {
    let own = own.ok()?;
    let (is_dir, size) = if !own.is_symlink {
        (own.is_dir, if own.is_dir { 0 } else { own.len })
    } else {
        match follow() {
            Ok(target) => (target.is_dir, if target.is_dir { 0 } else { target.len }),
            Err(_) => (false, 0),
        }
    };
    Some(RawDirEntry { name, is_dir, size, modified: own.modified, hidden: own.hidden, link: own.is_symlink })
}

/// The real filesystem, via `std::fs`, routed through [`to_long_path`].
#[derive(Debug, Default, Clone, Copy)]
pub struct StdFsReader;

/// The hidden/system flag for one enumerated entry, computed from the
/// enumeration's own metadata — never a per-file stat (hidden-entry-styling;
/// design D1). The synthetic `..` entry never passes through here; it is
/// built by [`Entry::parent_dir`] with `hidden: false`.
#[cfg(windows)]
fn is_hidden(_name: &std::ffi::OsStr, metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    metadata.file_attributes() & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0
}

/// The non-Windows rule: a leading `.` in the name (design D1).
#[cfg(not(windows))]
fn is_hidden(name: &std::ffi::OsStr, _metadata: &std::fs::Metadata) -> bool {
    name.to_string_lossy().starts_with('.')
}

/// The [`EntryFacts`] of one `std::fs::Metadata` record for the entry named
/// `name`. `is_symlink` is true for every name-surrogate reparse point on
/// Windows (junctions and symbolic links alike) and for symbolic links
/// elsewhere; `hidden` goes through the existing per-platform rule.
fn facts_of(name: &std::ffi::OsStr, metadata: &std::fs::Metadata) -> EntryFacts {
    EntryFacts {
        is_dir: metadata.is_dir(),
        is_symlink: metadata.file_type().is_symlink(),
        len: metadata.len(),
        modified: metadata.modified().ok(),
        hidden: is_hidden(name, metadata),
    }
}

impl FsReader for StdFsReader {
    /// Enumerate `path`. An error from the enumeration itself (`entry?`:
    /// `FindNextFileW`/`readdir` failing) is still returned as `Err` — it
    /// identifies no single entry, and continuing could repeat it forever.
    /// An entry whose own metadata cannot be read is skipped and counted
    /// instead of failing the whole listing (link-aware-listing design D6).
    /// A link is followed with one `std::fs::metadata` call so that a
    /// junction or symlink to a directory lists as a directory (design D1).
    fn read_dir(&self, path: &Path) -> io::Result<RawDirListing> {
        let long = to_long_path(path);
        let mut listing = RawDirListing::default();
        for entry in std::fs::read_dir(&long)? {
            let entry = entry?;
            let name = entry.file_name();
            let own = entry.metadata().map(|m| facts_of(&name, &m));
            let follow = || std::fs::metadata(long.join(&name)).map(|m| facts_of(&name, &m));
            match classify_entry(name.clone(), own, follow) {
                Some(raw) => listing.entries.push(raw),
                None => listing.unreadable += 1,
            }
        }
        Ok(listing)
    }
}

impl From<RawDirEntry> for Entry {
    fn from(raw: RawDirEntry) -> Self {
        Entry {
            name: raw.name,
            kind: if raw.is_dir { EntryKind::Directory } else { EntryKind::File },
            size: raw.size,
            modified: raw.modified.map(DateTime::from_system_time),
            hidden: raw.hidden,
            link: raw.link,
        }
    }
}

/// What [`list_dir_chunked`] delivered: the entries handed to `on_chunk`,
/// and the entries the reader skipped as unreadable (link-aware-listing
/// design D6). The caller reports the latter on the mini-status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListingTotals {
    pub entries: usize,
    pub unreadable: usize,
}

/// Read a directory and deliver entries in chunks via `on_chunk`, returning
/// the number delivered and the number the reader skipped as unreadable.
/// Designed to be called from a worker thread by `filecommand-tui`; core
/// never spawns threads itself. `cancel` is the listing's stop signal
/// (panel-navigation "Superseded reader stops"; listing-request-guard design
/// D5): it is checked before the read and before each chunk, and once set no
/// further chunk is delivered — the returned count then covers only what was
/// delivered. Until the streaming reader lands, the one `read_dir` call
/// itself still runs to completion; a superseded listing costs that
/// enumeration and nothing more.
pub fn list_dir_chunked<F: FnMut(Vec<Entry>)>(
    reader: &dyn FsReader,
    path: &Path,
    chunk_size: usize,
    cancel: &CancelFlag,
    mut on_chunk: F,
) -> io::Result<ListingTotals> {
    if cancel.is_cancelled() {
        return Ok(ListingTotals::default());
    }
    let raw = reader.read_dir(path)?;
    let mut totals = ListingTotals { entries: 0, unreadable: raw.unreadable };
    for chunk in raw.entries.chunks(chunk_size.max(1)) {
        if cancel.is_cancelled() {
            break;
        }
        let entries: Vec<Entry> = chunk.iter().cloned().map(Entry::from).collect();
        totals.entries += entries.len();
        on_chunk(entries);
    }
    Ok(totals)
}

// ---------------------------------------------------------------------
// Tree mode: lazy per-directory child reads (design D7)
// ---------------------------------------------------------------------

/// `path`'s immediate child directories only, sorted by name — used by Tree
/// display mode to expand exactly one node at a time. A directory that has
/// not been expanded is simply never passed to this function, so no
/// up-front full-drive scan ever happens (additional-panel-modes "Tree
/// lazy expansion"; design D7). Callers on `filecommand-tui` invoke this
/// once per node expansion, never eagerly for the whole tree.
pub fn list_child_dirs(reader: &dyn FsReader, path: &Path) -> io::Result<Vec<Entry>> {
    let raw = reader.read_dir(path)?;
    let mut dirs: Vec<Entry> = raw.entries.into_iter().filter(|e| e.is_dir).map(Entry::from).collect();
    dirs.sort_by(cmp_by_name);
    Ok(dirs)
}

// ---------------------------------------------------------------------
// Find-file (Alt+F7): recursive subtree name search (design D9)
// ---------------------------------------------------------------------

/// One matched entry from [`find_in_subtree`]: its location relative to the
/// search root (never absolute, so results display correctly however deep
/// the walk goes), and the matched entry itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindMatch {
    /// Path from the search root to (and including) the matched entry,
    /// e.g. `sub\deeper\name.txt` (find-file "Recursive subtree name
    /// search").
    pub relative_path: PathBuf,
    pub entry: Entry,
}

/// Walk `root`'s subtree depth-first via `reader`, delivering every entry
/// whose name contains `pattern` as a case-insensitive substring to
/// `on_match` as it is found. Matching is performed against each entry's
/// original name (via lossy display conversion, the same convention
/// `PanelState::select_matching`/`wildcard_match` already use elsewhere in
/// this crate), and non-Unicode names are matched and reported safely —
/// the returned [`FindMatch::entry`] always carries the original
/// `OsString`, so any later navigation never uses the display form
/// (find-file "Non-Unicode names are matched and displayed safely"). A
/// directory that fails to read partway through the walk (permission
/// denied, since-vanished directory) is skipped rather than aborting the
/// whole search. Every path traversed goes through `reader`, which for the
/// real filesystem routes through [`to_long_path`], so long paths are
/// handled correctly (design D9). This function performs the whole walk in
/// one call; `filecommand-tui` runs it on a worker thread and streams
/// `on_match` results back to the UI as they arrive so the walk never
/// blocks the UI thread (find-file "Non-blocking search with static
/// progress"). `cancel` is the walk's stop signal (find-file "Dismissed or
/// superseded searches stop"; listing-request-guard design D5): it is
/// checked before each directory read and between the entries examined,
/// and once set the walk returns without reading another directory or
/// reporting another match. A directory *link* is matched by name like any
/// entry but never descended into: links were leaves before they listed as
/// directories, and Windows ships junction cycles such as
/// `C:\ProgramData\Application Data` (link-aware-listing design D4).
pub fn find_in_subtree<F: FnMut(FindMatch)>(reader: &dyn FsReader, root: &Path, pattern: &str, cancel: &CancelFlag, mut on_match: F) {
    let needle = pattern.to_lowercase();
    // Relative sub-paths pending a read, root itself represented as "".
    let mut stack: Vec<PathBuf> = vec![PathBuf::new()];
    while let Some(rel_dir) = stack.pop() {
        if cancel.is_cancelled() {
            return;
        }
        let abs_dir = if rel_dir.as_os_str().is_empty() { root.to_path_buf() } else { root.join(&rel_dir) };
        let Ok(listing) = reader.read_dir(&abs_dir) else { continue };
        for raw in listing.entries {
            if cancel.is_cancelled() {
                return;
            }
            let rel_path = rel_dir.join(&raw.name);
            if !needle.is_empty() && raw.name.to_string_lossy().to_lowercase().contains(&needle) {
                on_match(FindMatch { relative_path: rel_path.clone(), entry: Entry::from(raw.clone()) });
            }
            if raw.is_dir && !raw.link {
                stack.push(rel_path);
            }
        }
    }
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
            let entry = Entry { name: raw.to_os_string(), kind: EntryKind::File, size: 0, modified: None, hidden: false, link: false };
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
        let dir = Entry { name: "sub".into(), kind: EntryKind::Directory, size: 0, modified: None, hidden: false, link: false };
        assert_eq!(entry_status_line(&dir), "sub  SUB-DIR");
        assert_eq!(entry_status_line(&Entry::parent_dir()), "..  UP--DIR");
    }

    #[test]
    fn entry_status_parts_splits_file_fields() {
        let dt = DateTime { year: 2026, month: 1, day: 2, hour: 3, minute: 4 };
        let file = Entry { name: "a.txt".into(), kind: EntryKind::File, size: 100, modified: Some(dt), hidden: false, link: false };
        let (name, size, date, time) = entry_status_parts(&file);
        assert_eq!(name, "a.txt");
        assert_eq!(size.as_deref(), Some("100"));
        assert_eq!(date.as_deref(), Some("01-02-26"));
        assert_eq!(time.as_deref(), Some("03:04"));
    }

    #[test]
    fn entry_status_parts_marks_dirs_and_parent_with_no_date_or_time() {
        let dir = Entry { name: "sub".into(), kind: EntryKind::Directory, size: 0, modified: None, hidden: false, link: false };
        assert_eq!(entry_status_parts(&dir), ("sub".to_string(), Some("SUB-DIR".to_string()), None, None));
        assert_eq!(entry_status_parts(&Entry::parent_dir()), ("..".to_string(), Some("UP--DIR".to_string()), None, None));
    }

    #[test]
    fn truncate_with_ellipsis_leaves_short_strings_untouched() {
        assert_eq!(truncate_with_ellipsis("abc", 5), "abc");
        assert_eq!(truncate_with_ellipsis("abc", 3), "abc");
    }

    #[test]
    fn truncate_with_ellipsis_appends_only_when_it_truncates() {
        assert_eq!(truncate_with_ellipsis("abcdef", 4), "abc\u{2026}");
        assert_eq!(display_width(&truncate_with_ellipsis("abcdef", 4)), 4);
    }

    #[test]
    fn truncate_with_ellipsis_handles_degenerate_widths() {
        assert_eq!(truncate_with_ellipsis("abc", 0), "");
        assert_eq!(truncate_with_ellipsis("abc", 1), "\u{2026}");
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
        fn read_dir(&self, _path: &Path) -> io::Result<RawDirListing> {
            Ok(RawDirListing { entries: self.0.clone(), unreadable: 0 })
        }
    }

    #[test]
    fn list_dir_chunked_delivers_chunks_and_total() {
        let entries: Vec<RawDirEntry> = (0..10)
            .map(|i| RawDirEntry { name: format!("f{i}").into(), is_dir: false, size: 0, modified: None, hidden: false, link: false })
            .collect();
        let reader = FakeReader(entries);
        let mut chunks = Vec::new();
        let total = list_dir_chunked(&reader, Path::new("."), 3, &CancelFlag::new(), |c| chunks.push(c.len())).unwrap();
        assert_eq!(total, ListingTotals { entries: 10, unreadable: 0 });
        assert_eq!(chunks, vec![3, 3, 3, 1]);
    }

    // -----------------------------------------------------------------
    // hidden-entry-styling: the hidden flag (tasks.md 1.3)
    // -----------------------------------------------------------------

    fn raw_hidden(name: &str, is_dir: bool) -> RawDirEntry {
        RawDirEntry { name: name.into(), is_dir, size: 0, modified: None, hidden: true, link: false }
    }

    #[test]
    fn from_raw_entry_copies_the_hidden_flag() {
        let file = Entry::from(raw_hidden("desktop.ini", false));
        assert!(file.hidden && file.kind == EntryKind::File);
        let dir = Entry::from(raw_hidden("System Volume Information", true));
        assert!(dir.hidden && dir.kind == EntryKind::Directory);
        assert!(!Entry::from(raw_file("readme.txt")).hidden);
    }

    #[test]
    fn parent_dir_is_never_hidden() {
        // panel-navigation "Entry row rendering": "the `..` entry is never
        // hidden" — the synthetic parent carries no attributes at all.
        assert!(!Entry::parent_dir().hidden);
    }

    #[test]
    fn hidden_flags_survive_enumeration_and_sorting() {
        // Entries read through the FsReader seam carry the flag into
        // `Entry`, and sorting reorders whole entries without dropping it
        // (hidden-entry-styling tasks.md 1.3).
        let reader = FakeReader(vec![raw_file("zeta.txt"), raw_hidden("desktop.ini", false), raw_hidden(".git", true), raw_dir("alpha")]);
        let mut entries = Vec::new();
        let total = list_dir_chunked(&reader, Path::new("."), 100, &CancelFlag::new(), |chunk| entries.extend(chunk)).unwrap();
        assert_eq!(total.entries, 4);
        let flags: Vec<(&str, bool)> = entries.iter().map(|e| (e.name.to_str().unwrap(), e.hidden)).collect();
        assert_eq!(flags, vec![("zeta.txt", false), ("desktop.ini", true), (".git", true), ("alpha", false)], "enumeration order");

        entries.sort_by(|a, b| cmp_by_mode(a, b, SortMode::Name));
        let flags: Vec<(&str, bool)> = entries.iter().map(|e| (e.name.to_str().unwrap(), e.hidden)).collect();
        assert_eq!(flags, vec![(".git", true), ("alpha", false), ("desktop.ini", true), ("zeta.txt", false)], "the flag follows its entry through the sort");
    }

    // -----------------------------------------------------------------
    // Tree lazy child reads and find-file subtree walk (task 15.8)
    // -----------------------------------------------------------------

    /// A reader whose answer depends on the directory asked for, so tests
    /// can model a small multi-level tree without touching the real
    /// filesystem.
    struct TreeReader(std::collections::HashMap<PathBuf, Vec<RawDirEntry>>);

    impl FsReader for TreeReader {
        fn read_dir(&self, path: &Path) -> io::Result<RawDirListing> {
            self.0
                .get(path)
                .cloned()
                .map(|entries| RawDirListing { entries, unreadable: 0 })
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such fixture directory"))
        }
    }

    fn raw_dir(name: &str) -> RawDirEntry {
        RawDirEntry { name: name.into(), is_dir: true, size: 0, modified: None, hidden: false, link: false }
    }
    fn raw_file(name: &str) -> RawDirEntry {
        RawDirEntry { name: name.into(), is_dir: false, size: 0, modified: None, hidden: false, link: false }
    }

    #[test]
    fn list_child_dirs_reads_only_the_requested_directory_and_only_dirs() {
        let mut fixture = std::collections::HashMap::new();
        fixture.insert(PathBuf::from("/root"), vec![raw_dir("sub_a"), raw_file("readme.txt"), raw_dir("sub_b")]);
        // A deeper directory exists in the fixture but must never be read
        // by this call — expanding "/root" must not touch its children.
        fixture.insert(PathBuf::from("/root/sub_a"), vec![raw_dir("deeper")]);
        let reader = TreeReader(fixture);

        let children = list_child_dirs(&reader, Path::new("/root")).unwrap();
        let names: Vec<String> = children.iter().map(|e| e.name.to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["sub_a", "sub_b"], "only directories, sorted by name, no-up-front deeper read");
        assert!(children.iter().all(|e| e.kind == EntryKind::Directory));
    }

    #[test]
    fn list_child_dirs_on_an_unexpanded_node_is_simply_never_called() {
        // Tree lazy expansion means an unexpanded directory shows no
        // children because `filecommand-tui` never calls `list_child_dirs`
        // for it in the first place — there is nothing for `listing` to
        // assert here beyond `list_child_dirs` itself doing exactly one
        // directory's worth of I/O per call, which the test above already
        // covers (additional-panel-modes "Unexpanded directory shows no
        // children").
        let mut fixture = std::collections::HashMap::new();
        fixture.insert(PathBuf::from("/root"), vec![raw_dir("sub")]);
        let reader = TreeReader(fixture);
        let root_children = list_child_dirs(&reader, Path::new("/root")).unwrap();
        assert_eq!(root_children.len(), 1);
        // "/root/sub" was deliberately never inserted into the fixture, so
        // a premature read attempt would fail loudly rather than silently
        // succeeding with wrong data.
    }

    #[test]
    fn find_in_subtree_matches_nested_entries_with_relative_locations() {
        let mut fixture = std::collections::HashMap::new();
        fixture.insert(PathBuf::from("/root"), vec![raw_dir("sub"), raw_file("report.txt")]);
        fixture.insert(PathBuf::from("/root/sub"), vec![raw_dir("deeper"), raw_file("report_copy.txt")]);
        fixture.insert(PathBuf::from("/root/sub/deeper"), vec![raw_file("final_report.txt"), raw_file("unrelated.md")]);
        let reader = TreeReader(fixture);

        let mut matches = Vec::new();
        find_in_subtree(&reader, Path::new("/root"), "report", &CancelFlag::new(), |m| matches.push(m));
        matches.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

        let paths: Vec<PathBuf> = matches.iter().map(|m| m.relative_path.clone()).collect();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("report.txt"),
                PathBuf::from("sub").join("deeper").join("final_report.txt"),
                PathBuf::from("sub").join("report_copy.txt"),
            ]
        );
        assert!(!paths.contains(&PathBuf::from("sub").join("deeper").join("unrelated.md")));
    }

    #[test]
    fn find_in_subtree_matches_are_case_insensitive_and_use_the_original_name() {
        let mut fixture = std::collections::HashMap::new();
        fixture.insert(PathBuf::from("/root"), vec![raw_file("ReadMe.MD")]);
        let reader = TreeReader(fixture);

        let mut matches = Vec::new();
        find_in_subtree(&reader, Path::new("/root"), "readme", &CancelFlag::new(), |m| matches.push(m));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].entry.name, OsString::from("ReadMe.MD"), "the returned entry must keep the original OsString name");
    }

    #[test]
    fn find_in_subtree_no_matches_yields_an_empty_result() {
        let mut fixture = std::collections::HashMap::new();
        fixture.insert(PathBuf::from("/root"), vec![raw_file("a.txt"), raw_dir("sub")]);
        fixture.insert(PathBuf::from("/root/sub"), vec![raw_file("b.txt")]);
        let reader = TreeReader(fixture);

        let mut matches = Vec::new();
        find_in_subtree(&reader, Path::new("/root"), "nonexistent-pattern", &CancelFlag::new(), |m| matches.push(m));
        assert!(matches.is_empty());
    }

    #[test]
    fn find_in_subtree_skips_an_unreadable_directory_without_aborting_the_walk() {
        let mut fixture = std::collections::HashMap::new();
        // "/root/broken" is a directory entry but deliberately has no
        // fixture data of its own, so `TreeReader::read_dir` fails for it —
        // the walk must skip past that failure and keep going rather than
        // losing the rest of the subtree.
        fixture.insert(PathBuf::from("/root"), vec![raw_dir("broken"), raw_dir("ok")]);
        fixture.insert(PathBuf::from("/root/ok"), vec![raw_file("target_match.txt")]);
        let reader = TreeReader(fixture);

        let mut matches = Vec::new();
        find_in_subtree(&reader, Path::new("/root"), "target", &CancelFlag::new(), |m| matches.push(m));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].relative_path, PathBuf::from("ok").join("target_match.txt"));
    }

    // -----------------------------------------------------------------
    // listing-request-guard: the stop signal (find-file "Esc stops the
    // walk"; panel-navigation "Superseded reader stops"; design D5)
    // -----------------------------------------------------------------

    /// A reader that counts how many directories it has been asked to read,
    /// so a test can show the walk stopped asking once cancelled.
    struct CountingReader {
        inner: TreeReader,
        reads: std::cell::Cell<usize>,
    }

    impl FsReader for CountingReader {
        fn read_dir(&self, path: &Path) -> io::Result<RawDirListing> {
            self.reads.set(self.reads.get() + 1);
            self.inner.read_dir(path)
        }
    }

    #[test]
    fn find_in_subtree_stops_reading_directories_once_cancelled_from_inside_on_match() {
        // Three levels, each with a match and a subdirectory below it. The
        // flag is set from inside the first `on_match`, as the TUI would
        // when the reducer's `CancelRequest` lands mid-walk: no further
        // directory is read and no further match is reported.
        let mut fixture = std::collections::HashMap::new();
        fixture.insert(PathBuf::from("/root"), vec![raw_file("report1.txt"), raw_dir("a")]);
        fixture.insert(PathBuf::from("/root/a"), vec![raw_file("report2.txt"), raw_dir("b")]);
        fixture.insert(PathBuf::from("/root/a/b"), vec![raw_file("report3.txt")]);
        let reader = CountingReader { inner: TreeReader(fixture), reads: std::cell::Cell::new(0) };

        let cancel = CancelFlag::new();
        let mut matches = Vec::new();
        find_in_subtree(&reader, Path::new("/root"), "report", &cancel, |m| {
            matches.push(m);
            cancel.cancel();
        });
        assert_eq!(matches.len(), 1, "nothing is reported after the flag is set");
        assert_eq!(reader.reads.get(), 1, "no directory is read after the flag is set");
    }

    #[test]
    fn find_in_subtree_with_a_pre_set_flag_reads_nothing() {
        let mut fixture = std::collections::HashMap::new();
        fixture.insert(PathBuf::from("/root"), vec![raw_file("report.txt")]);
        let reader = CountingReader { inner: TreeReader(fixture), reads: std::cell::Cell::new(0) };
        let cancel = CancelFlag::new();
        cancel.cancel();
        let mut matches = Vec::new();
        find_in_subtree(&reader, Path::new("/root"), "report", &cancel, |m| matches.push(m));
        assert!(matches.is_empty());
        assert_eq!(reader.reads.get(), 0, "the check precedes the first directory read");
    }

    #[test]
    fn list_dir_chunked_with_a_pre_set_flag_delivers_nothing() {
        let entries: Vec<RawDirEntry> = (0..10).map(|i| raw_file(&format!("f{i}"))).collect();
        let reader = FakeReader(entries);
        let cancel = CancelFlag::new();
        cancel.cancel();
        let mut chunks = 0usize;
        let total = list_dir_chunked(&reader, Path::new("."), 3, &cancel, |_| chunks += 1).unwrap();
        assert_eq!(chunks, 0, "a superseded reader delivers no chunk");
        assert_eq!(total.entries, 0);
    }

    #[test]
    fn list_dir_chunked_stops_delivering_once_cancelled_between_chunks() {
        let entries: Vec<RawDirEntry> = (0..10).map(|i| raw_file(&format!("f{i}"))).collect();
        let reader = FakeReader(entries);
        let cancel = CancelFlag::new();
        let mut chunks = Vec::new();
        let total = list_dir_chunked(&reader, Path::new("."), 3, &cancel, |c| {
            chunks.push(c.len());
            cancel.cancel();
        })
        .unwrap();
        assert_eq!(chunks, vec![3], "the chunk in flight when the flag was set is the last one");
        assert_eq!(total.entries, 3, "the count covers only what was delivered");
    }

    // -----------------------------------------------------------------
    // link-aware-listing: link classification and skipped entries
    // (panel-navigation "Directory links list as directories", "One
    // unreadable entry does not fail a listing"; design D1/D2/D6/D8)
    // -----------------------------------------------------------------

    fn facts(is_dir: bool, is_symlink: bool, len: u64, secs: u64, hidden: bool) -> EntryFacts {
        EntryFacts { is_dir, is_symlink, len, modified: Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs)), hidden }
    }

    fn not_followed() -> io::Result<EntryFacts> {
        panic!("a non-link entry must never be followed")
    }

    #[test]
    fn classify_entry_skips_an_entry_whose_own_metadata_cannot_be_read() {
        let own = Err(io::Error::new(io::ErrorKind::NotFound, "raced deletion"));
        assert_eq!(classify_entry("gone".into(), own, not_followed), None);
    }

    #[test]
    fn classify_entry_keeps_a_plain_file_and_a_plain_directory_as_they_are() {
        let file = classify_entry("a.txt".into(), Ok(facts(false, false, 42, 10, false)), not_followed).unwrap();
        assert_eq!(file, RawDirEntry { name: "a.txt".into(), is_dir: false, size: 42, modified: facts(false, false, 42, 10, false).modified, hidden: false, link: false });

        let dir = classify_entry("sub".into(), Ok(facts(true, false, 4096, 20, true)), not_followed).unwrap();
        assert!(dir.is_dir && !dir.link && dir.hidden);
        assert_eq!(dir.size, 0, "a directory's own length is never shown as a size");
    }

    #[test]
    fn classify_entry_lists_a_link_to_a_directory_as_a_directory_with_the_links_own_date_and_hidden_flag() {
        let link_facts = facts(false, true, 0, 10, true);
        let target = facts(true, false, 4096, 99, false);
        let raw = classify_entry("lodash".into(), Ok(link_facts), || Ok(target)).unwrap();
        assert!(raw.is_dir, "classified by the target");
        assert!(raw.link);
        assert_eq!(raw.size, 0);
        assert_eq!(raw.modified, link_facts.modified, "the date is the link's own");
        assert!(raw.hidden, "the hidden flag is the link's own");
    }

    #[test]
    fn classify_entry_lists_a_link_to_a_file_with_the_targets_size_and_the_links_date() {
        let link_facts = facts(false, true, 0, 10, false);
        let target = facts(false, false, 4096, 99, false);
        let raw = classify_entry("notes.lnk".into(), Ok(link_facts), || Ok(target)).unwrap();
        assert!(!raw.is_dir && raw.link);
        assert_eq!(raw.size, 4096, "the Size column shows the target's size");
        assert_eq!(raw.modified, link_facts.modified);
    }

    #[test]
    fn classify_entry_lists_a_dangling_link_as_a_non_enterable_file_without_failing() {
        let link_facts = facts(false, true, 0, 10, false);
        let raw = classify_entry("old".into(), Ok(link_facts), || Err(io::Error::new(io::ErrorKind::NotFound, "target gone"))).unwrap();
        assert_eq!(raw, RawDirEntry { name: "old".into(), is_dir: false, size: 0, modified: link_facts.modified, hidden: false, link: true });
        // A target that exists but cannot be reached is treated the same.
        let raw = classify_entry("denied".into(), Ok(link_facts), || Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))).unwrap();
        assert!(!raw.is_dir && raw.link && raw.size == 0);
    }

    /// A reader that also reports skipped entries, for the unreadable count.
    struct SkippingReader(RawDirListing);
    impl FsReader for SkippingReader {
        fn read_dir(&self, _path: &Path) -> io::Result<RawDirListing> {
            Ok(self.0.clone())
        }
    }

    fn raw_dir_link(name: &str) -> RawDirEntry {
        RawDirEntry { name: name.into(), is_dir: true, size: 0, modified: None, hidden: false, link: true }
    }

    #[test]
    fn list_dir_chunked_reports_the_skipped_count_and_the_link_flag_survives_conversion_and_sorting() {
        let reader = SkippingReader(RawDirListing { entries: vec![raw_file("zeta.txt"), raw_dir_link("lnk"), raw_dir("alpha")], unreadable: 2 });
        let mut entries = Vec::new();
        let totals = list_dir_chunked(&reader, Path::new("."), 100, &CancelFlag::new(), |chunk| entries.extend(chunk)).unwrap();
        assert_eq!(totals, ListingTotals { entries: 3, unreadable: 2 });

        let flags: Vec<(&str, EntryKind, bool)> = entries.iter().map(|e| (e.name.to_str().unwrap(), e.kind, e.link)).collect();
        assert_eq!(flags, vec![("zeta.txt", EntryKind::File, false), ("lnk", EntryKind::Directory, true), ("alpha", EntryKind::Directory, false)]);

        entries.sort_by(|a, b| cmp_by_mode(a, b, SortMode::Name));
        let flags: Vec<(&str, bool)> = entries.iter().map(|e| (e.name.to_str().unwrap(), e.link)).collect();
        assert_eq!(flags, vec![("alpha", false), ("lnk", true), ("zeta.txt", false)], "a directory link sorts among the directories and keeps its flag");
    }

    #[test]
    fn parent_dir_is_never_a_link() {
        assert!(!Entry::parent_dir().link);
    }

    #[test]
    fn find_in_subtree_matches_a_directory_link_by_name_but_never_reads_beneath_it() {
        // design D4: links are leaves for the walk — `report_link` is
        // reported, `/root/report_link/report_b.txt` is never seen, and the
        // directory behind the link is never read.
        let mut fixture = std::collections::HashMap::new();
        fixture.insert(PathBuf::from("/root"), vec![raw_dir("real"), raw_dir_link("report_link")]);
        fixture.insert(PathBuf::from("/root/real"), vec![raw_file("report_a.txt")]);
        fixture.insert(PathBuf::from("/root/report_link"), vec![raw_file("report_b.txt")]);
        let reader = CountingReader { inner: TreeReader(fixture), reads: std::cell::Cell::new(0) };

        let mut paths = Vec::new();
        find_in_subtree(&reader, Path::new("/root"), "report", &CancelFlag::new(), |m| paths.push(m.relative_path));
        paths.sort();
        assert_eq!(paths, vec![PathBuf::from("real").join("report_a.txt"), PathBuf::from("report_link")]);
        assert_eq!(reader.reads.get(), 2, "only /root and /root/real are read");
    }

    /// Real junctions on the real filesystem (tasks.md 4.4): `mklink /J`
    /// needs no privilege. The junction to a live directory lists as a
    /// directory, the dangling one as a file, and neither fails the read
    /// (panel-navigation "A junction to a directory lists as a directory",
    /// "A dangling link does not fail the listing").
    #[cfg(windows)]
    #[test]
    fn std_reader_classifies_junctions_by_their_target_and_survives_a_dangling_one() {
        let dir = std::env::temp_dir().join(format!("filecommand-listing-junction-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::write(dir.join("target").join("inside.txt"), b"x").unwrap();
        std::fs::write(dir.join("plain.txt"), b"12345").unwrap();
        std::fs::create_dir(dir.join("gone")).unwrap();
        let mklink = |link: &str, target: &str| {
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J", link, target])
                .current_dir(&dir)
                .stdout(std::process::Stdio::null())
                .status()
                .expect("cmd.exe runs");
            assert!(status.success(), "mklink /J {link} {target} failed");
        };
        mklink("lnk", "target");
        mklink("old", "gone");
        std::fs::remove_dir(dir.join("gone")).unwrap(); // `old` now dangles

        let listing = StdFsReader.read_dir(&dir).expect("a dangling junction does not fail the listing");
        assert_eq!(listing.unreadable, 0);
        let by_name = |name: &str| listing.entries.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{name} is listed: {:?}", listing.entries)).clone();
        let lnk = by_name("lnk");
        assert!(lnk.is_dir && lnk.link, "a junction to a directory lists as a directory: {lnk:?}");
        let old = by_name("old");
        assert!(!old.is_dir && old.link && old.size == 0, "a dangling junction lists as a non-enterable file: {old:?}");
        let target = by_name("target");
        assert!(target.is_dir && !target.link);
        let plain = by_name("plain.txt");
        assert!(!plain.is_dir && !plain.link && plain.size == 5);
        assert_eq!(listing.entries.len(), 4);

        // The junction's own path lists the target's contents, as Enter would.
        let through = StdFsReader.read_dir(&dir.join("lnk")).unwrap();
        assert_eq!(through.entries.iter().map(|e| e.name.clone()).collect::<Vec<_>>(), vec![OsString::from("inside.txt")]);

        let _ = std::fs::remove_dir(dir.join("lnk"));
        let _ = std::fs::remove_dir(dir.join("old"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Symbolic links on Unix (tasks.md 4.5): to a directory, to a
    /// 4,096-byte file, and to a missing path (panel-navigation "A directory
    /// symbolic link behaves like a junction", "A link to a file lists as a
    /// file with the target's size", "A dangling link does not fail the
    /// listing").
    #[cfg(unix)]
    #[test]
    fn std_reader_classifies_symlinks_by_their_target_and_survives_a_dangling_one() {
        let dir = std::env::temp_dir().join(format!("filecommand-listing-symlink-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::write(dir.join("big.bin"), vec![0u8; 4096]).unwrap();
        std::os::unix::fs::symlink(dir.join("target"), dir.join("dir_link")).unwrap();
        std::os::unix::fs::symlink(dir.join("big.bin"), dir.join("file_link")).unwrap();
        std::os::unix::fs::symlink(dir.join("missing"), dir.join("dangling")).unwrap();

        let listing = StdFsReader.read_dir(&dir).expect("a dangling symlink does not fail the listing");
        assert_eq!(listing.unreadable, 0);
        let by_name = |name: &str| listing.entries.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{name} is listed")).clone();
        let dir_link = by_name("dir_link");
        assert!(dir_link.is_dir && dir_link.link);
        let file_link = by_name("file_link");
        assert!(!file_link.is_dir && file_link.link);
        assert_eq!(file_link.size, 4096, "the target's size");
        let dangling = by_name("dangling");
        assert!(!dangling.is_dir && dangling.link && dangling.size == 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn find_in_subtree_matches_non_unicode_names_via_lossy_substring() {
        use std::os::unix::ffi::OsStrExt;
        let mut fixture = std::collections::HashMap::new();
        // "fo\x80report" — an invalid UTF-8 byte sits before an otherwise
        // matchable ASCII substring.
        let mut raw_name = vec![0x66, 0x6f, 0x80];
        raw_name.extend_from_slice(b"report.txt");
        let name = std::ffi::OsStr::from_bytes(&raw_name).to_os_string();
        fixture.insert(PathBuf::from("/root"), vec![RawDirEntry { name: name.clone(), is_dir: false, size: 0, modified: None, hidden: false, link: false }]);
        let reader = TreeReader(fixture);

        let mut matches = Vec::new();
        find_in_subtree(&reader, Path::new("/root"), "report", &CancelFlag::new(), |m| matches.push(m));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].entry.name, name, "matching must not corrupt the original OsString even though it went through lossy conversion");
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

            /// The new M5 fs paths (Tree's per-level `list_child_dirs`
            /// expansion, the editor's load/save path, find-file's
            /// subtree walk) all reach the filesystem through repeated
            /// `Path::join` before a single `to_long_path` call, mirroring
            /// how `find_in_subtree`'s stack-based walk and Tree's
            /// node-by-node expansion build up a path one segment at a
            /// time. Arbitrarily deep joining must stay correctly
            /// `\\?\`-prefixed and must not lose or reorder segments.
            #[test]
            fn to_long_path_handles_arbitrarily_deep_joins(segments in prop::collection::vec("[a-zA-Z0-9_]{1,8}", 1..6)) {
                #[cfg(windows)]
                {
                    let mut joined = PathBuf::from(r"C:\");
                    for seg in &segments {
                        joined = joined.join(seg);
                    }
                    let long = to_long_path(&joined);
                    let shown = long.to_string_lossy();
                    prop_assert!(shown.starts_with(r"\\?\"));
                    let suffix = segments.join(r"\");
                    prop_assert!(shown.ends_with(&suffix));
                    // Idempotent even after the deep join, same as the
                    // single-level case.
                    prop_assert_eq!(to_long_path(&long), long);
                }
                #[cfg(not(windows))]
                {
                    let mut joined = PathBuf::from("/");
                    for seg in &segments {
                        joined = joined.join(seg);
                    }
                    prop_assert_eq!(to_long_path(&joined), joined);
                }
            }
        }
    }
}
