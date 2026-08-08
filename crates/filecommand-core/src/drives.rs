//! Drive enumeration and per-drive metadata, behind a thin platform seam.
//!
//! Enumerating drive *letters* is cheap (`GetLogicalDrives` is a bitmask
//! read) and is done synchronously so the drive-select dialog paints its
//! full list on the first frame. Everything else — volume label, serial,
//! total/free bytes — can block on absent media or an unreachable network
//! share, so those are only ever called from a worker thread.
//!
//! The Windows calls are declared as direct `kernel32` FFI rather than
//! pulling in a bindings crate: M3 needs five entry points and the crate
//! otherwise has a single dependency.

use std::path::{Path, PathBuf};

use crate::PanelSide;

/// One row of the drive-select dialog. `label` is `None` until the lazy
/// worker fetch resolves, which is what renders the label column blank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveEntry {
    pub letter: char,
    pub label: Option<String>,
}

/// The open drive-select dialog (Alt+F1/F2). Letters are known up front;
/// labels arrive later and fill in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveSelect {
    /// Which panel the chosen drive will be applied to — fixed at open time
    /// by whether Alt+F1 or Alt+F2 was pressed, not by which panel is
    /// focused.
    pub target: PanelSide,
    pub drives: Vec<DriveEntry>,
    pub selected: usize,
}

impl DriveSelect {
    /// Open a dialog listing `letters`, with the cursor on `current` when
    /// that drive is present.
    pub fn new(target: PanelSide, letters: Vec<char>, current: Option<char>) -> DriveSelect {
        let drives: Vec<DriveEntry> = letters.into_iter().map(|letter| DriveEntry { letter, label: None }).collect();
        let selected = current
            .and_then(|c| drives.iter().position(|d| d.letter.eq_ignore_ascii_case(&c)))
            .unwrap_or(0);
        DriveSelect { target, drives, selected }
    }

    pub fn selected_letter(&self) -> Option<char> {
        self.drives.get(self.selected).map(|d| d.letter)
    }

    /// Move the highlight, clamping at both ends (the drive list is short
    /// enough that wrapping is more disorienting than helpful).
    pub fn move_selection(&mut self, delta: isize) {
        if self.drives.is_empty() {
            self.selected = 0;
            return;
        }
        let last = self.drives.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, last) as usize;
    }

    /// Fill in a resolved label without disturbing any other row's position
    /// or letter. A letter no longer in the list is silently ignored.
    pub fn apply_label(&mut self, letter: char, label: Option<String>) {
        if let Some(entry) = self.drives.iter_mut().find(|d| d.letter == letter) {
            entry.label = Some(label.unwrap_or_default());
        }
    }
}

/// Total and free bytes for a volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskSpace {
    pub total: u64,
    pub free: u64,
}

/// Turn the `GetLogicalDrives` bitmask into drive letters. Bit 0 is `A:`.
/// Pure, so the bit-twiddling is testable without a Windows host.
pub fn drives_from_bitmask(mask: u32) -> Vec<char> {
    (0..26u32)
        .filter(|i| mask & (1 << i) != 0)
        .filter_map(|i| char::from_u32(u32::from(b'A') + i))
        .collect()
}

/// The root directory of a drive letter, e.g. `C:\`.
pub fn drive_root(letter: char) -> PathBuf {
    PathBuf::from(format!("{}:\\", letter.to_ascii_uppercase()))
}

/// Whether `path` is a UNC path (`\\server\share`). UNC targets are entered
/// manually and are valid panel targets, so they must not be mistaken for a
/// malformed drive path.
pub fn is_unc_path(path: &Path) -> bool {
    let s = path.as_os_str().to_string_lossy();
    let s = s.strip_prefix(r"\\?\UNC\").map(|rest| format!(r"\\{rest}")).unwrap_or_else(|| s.into_owned());
    (s.starts_with(r"\\") || s.starts_with("//")) && s.len() > 2
}

/// The drive letter a path lives on, or `None` for UNC and relative paths.
pub fn drive_letter_of(path: &Path) -> Option<char> {
    let s = path.as_os_str().to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    let mut chars = s.chars();
    let letter = chars.next()?;
    if chars.next() != Some(':') || !letter.is_ascii_alphabetic() {
        return None;
    }
    Some(letter.to_ascii_uppercase())
}

#[cfg(windows)]
mod platform {
    use super::DiskSpace;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    /// Suppress the "There is no disk in the drive" system modal — without
    /// this, probing an empty `A:` pops a dialog the user must dismiss.
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;

    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetLogicalDrives() -> u32;
        fn SetThreadErrorMode(new_mode: u32, old_mode: *mut u32) -> i32;
        fn GetVolumeInformationW(
            root_path: *const u16,
            volume_name: *mut u16,
            volume_name_size: u32,
            serial: *mut u32,
            max_component_len: *mut u32,
            fs_flags: *mut u32,
            fs_name: *mut u16,
            fs_name_size: u32,
        ) -> i32;
        fn GetDiskFreeSpaceExW(directory: *const u16, free_to_caller: *mut u64, total: *mut u64, total_free: *mut u64) -> i32;
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Run `f` with the critical-error dialog suppressed for this thread.
    fn quietly<T>(f: impl FnOnce() -> T) -> T {
        let mut previous = 0u32;
        let changed = unsafe { SetThreadErrorMode(SEM_FAILCRITICALERRORS, &mut previous) } != 0;
        let out = f();
        if changed {
            unsafe { SetThreadErrorMode(previous, std::ptr::null_mut()) };
        }
        out
    }

    pub fn enumerate_drives() -> Vec<char> {
        super::drives_from_bitmask(unsafe { GetLogicalDrives() })
    }

    /// `(label, serial)` for a drive, or `None` when the volume can't be
    /// read (no media, disconnected share).
    pub fn volume_info(letter: char) -> Option<(String, String)> {
        let root = wide(&format!("{}:\\", letter.to_ascii_uppercase()));
        let mut name = [0u16; 261];
        let mut serial = 0u32;
        let ok = quietly(|| unsafe {
            GetVolumeInformationW(
                root.as_ptr(),
                name.as_mut_ptr(),
                name.len() as u32,
                &mut serial,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            )
        });
        if ok == 0 {
            return None;
        }
        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        let label = String::from_utf16_lossy(&name[..len]);
        Some((label, format_serial(serial)))
    }

    pub fn disk_space(letter: char) -> Option<DiskSpace> {
        let root = wide(&format!("{}:\\", letter.to_ascii_uppercase()));
        let mut free_to_caller = 0u64;
        let mut total = 0u64;
        let mut total_free = 0u64;
        let ok = quietly(|| unsafe { GetDiskFreeSpaceExW(root.as_ptr(), &mut free_to_caller, &mut total, &mut total_free) });
        if ok == 0 {
            return None;
        }
        Some(DiskSpace { total, free: total_free })
    }

    pub fn available_memory() -> Option<u64> {
        let mut status = MemoryStatusEx {
            length: std::mem::size_of::<MemoryStatusEx>() as u32,
            memory_load: 0,
            total_phys: 0,
            avail_phys: 0,
            total_page_file: 0,
            avail_page_file: 0,
            total_virtual: 0,
            avail_virtual: 0,
            avail_extended_virtual: 0,
        };
        if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
            return None;
        }
        Some(status.avail_phys)
    }

    /// NC renders volume serials as `XXXX-XXXX` hex.
    pub fn format_serial(serial: u32) -> String {
        format!("{:04X}-{:04X}", serial >> 16, serial & 0xFFFF)
    }
}

#[cfg(not(windows))]
mod platform {
    use super::DiskSpace;

    /// Non-Windows hosts have no drive letters. The dialog still opens and
    /// renders (empty), and every caller already handles the empty case,
    /// so cross-platform builds stay best-effort rather than broken.
    pub fn enumerate_drives() -> Vec<char> {
        Vec::new()
    }

    pub fn volume_info(_letter: char) -> Option<(String, String)> {
        None
    }

    pub fn disk_space(_letter: char) -> Option<DiskSpace> {
        None
    }

    pub fn available_memory() -> Option<u64> {
        None
    }

    pub fn format_serial(serial: u32) -> String {
        format!("{:04X}-{:04X}", serial >> 16, serial & 0xFFFF)
    }
}

/// Every drive letter currently present, in `A:`..`Z:` order. Cheap enough
/// to call synchronously on the input path.
pub fn enumerate_drives() -> Vec<char> {
    platform::enumerate_drives()
}

/// `(volume label, serial)` for a drive. **Worker threads only** — this can
/// block on absent media or an unreachable network share.
pub fn volume_info(letter: char) -> Option<(String, String)> {
    platform::volume_info(letter)
}

/// Total/free bytes for a drive. **Worker threads only** — same blocking
/// caveat as [`volume_info`].
pub fn disk_space(letter: char) -> Option<DiskSpace> {
    platform::disk_space(letter)
}

/// Available physical memory in bytes, for the Info panel's memory field.
pub fn available_memory() -> Option<u64> {
    platform::available_memory()
}

/// Format a volume serial as NC does: `XXXX-XXXX`.
pub fn format_serial(serial: u32) -> String {
    platform::format_serial(serial)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmask_maps_bits_to_letters() {
        assert_eq!(drives_from_bitmask(0), Vec::<char>::new());
        assert_eq!(drives_from_bitmask(0b1), vec!['A']);
        // A:, C:, D:
        assert_eq!(drives_from_bitmask(0b1101), vec!['A', 'C', 'D']);
        assert_eq!(drives_from_bitmask(u32::MAX).len(), 26, "only 26 letters exist regardless of high bits");
        assert_eq!(*drives_from_bitmask(u32::MAX).last().unwrap(), 'Z');
    }

    #[test]
    fn drive_root_is_letter_colon_backslash() {
        assert_eq!(drive_root('c'), PathBuf::from(r"C:\"));
        assert_eq!(drive_root('D'), PathBuf::from(r"D:\"));
    }

    #[test]
    fn unc_paths_are_recognized_and_have_no_drive_letter() {
        assert!(is_unc_path(Path::new(r"\\server\share")));
        assert!(is_unc_path(Path::new(r"\\server\share\sub")));
        assert!(is_unc_path(Path::new(r"\\?\UNC\server\share")));
        assert!(!is_unc_path(Path::new(r"C:\Users")));
        assert!(!is_unc_path(Path::new(r"\\")));
        assert_eq!(drive_letter_of(Path::new(r"\\server\share")), None);
    }

    #[test]
    fn drive_letter_extracted_from_local_and_long_paths() {
        assert_eq!(drive_letter_of(Path::new(r"C:\Users\demo")), Some('C'));
        assert_eq!(drive_letter_of(Path::new(r"d:\work")), Some('D'));
        assert_eq!(drive_letter_of(Path::new(r"\\?\E:\stuff")), Some('E'));
        assert_eq!(drive_letter_of(Path::new("relative/path")), None);
        assert_eq!(drive_letter_of(Path::new("/unix/path")), None);
    }

    #[test]
    fn serial_formats_as_nc_style_hex_pairs() {
        assert_eq!(format_serial(0x1A2B_3C4D), "1A2B-3C4D");
        assert_eq!(format_serial(0), "0000-0000");
    }

    #[test]
    fn dialog_lists_every_letter_with_blank_labels_up_front() {
        let dialog = DriveSelect::new(PanelSide::Left, vec!['A', 'C', 'D'], None);
        assert_eq!(dialog.drives.len(), 3);
        assert!(dialog.drives.iter().all(|d| d.label.is_none()), "no label is known on the first frame");
        assert_eq!(dialog.selected_letter(), Some('A'));
    }

    #[test]
    fn dialog_opens_with_the_cursor_on_the_panel_s_current_drive() {
        let dialog = DriveSelect::new(PanelSide::Right, vec!['A', 'C', 'D'], Some('d'));
        assert_eq!(dialog.selected_letter(), Some('D'));
    }

    #[test]
    fn label_fills_in_place_leaving_other_rows_untouched() {
        let mut dialog = DriveSelect::new(PanelSide::Left, vec!['A', 'C', 'D'], None);
        dialog.apply_label('C', Some("OS".to_string()));
        assert_eq!(dialog.drives[0], DriveEntry { letter: 'A', label: None });
        assert_eq!(dialog.drives[1], DriveEntry { letter: 'C', label: Some("OS".to_string()) });
        assert_eq!(dialog.drives[2], DriveEntry { letter: 'D', label: None });
    }

    #[test]
    fn an_unlabelled_volume_resolves_to_an_empty_label_not_a_pending_one() {
        let mut dialog = DriveSelect::new(PanelSide::Left, vec!['C'], None);
        dialog.apply_label('C', None);
        assert_eq!(dialog.drives[0].label, Some(String::new()), "a resolved-but-blank label is not still pending");
    }

    #[test]
    fn label_for_a_letter_not_listed_is_ignored() {
        let mut dialog = DriveSelect::new(PanelSide::Left, vec!['C'], None);
        dialog.apply_label('Z', Some("Zip".to_string()));
        assert_eq!(dialog.drives[0].label, None);
    }

    #[test]
    fn selection_clamps_at_both_ends() {
        let mut dialog = DriveSelect::new(PanelSide::Left, vec!['A', 'C', 'D'], None);
        dialog.move_selection(-1);
        assert_eq!(dialog.selected, 0);
        dialog.move_selection(99);
        assert_eq!(dialog.selected, 2);
        dialog.move_selection(1);
        assert_eq!(dialog.selected, 2);
    }

    #[test]
    fn empty_drive_list_has_no_selection_and_does_not_panic() {
        let mut dialog = DriveSelect::new(PanelSide::Left, vec![], None);
        dialog.move_selection(1);
        assert_eq!(dialog.selected_letter(), None);
    }

    #[test]
    fn enumerate_drives_is_callable_and_ordered() {
        let drives = enumerate_drives();
        for w in drives.windows(2) {
            assert!(w[0] < w[1], "drive letters must come back in ascending order");
        }
        #[cfg(windows)]
        assert!(!drives.is_empty(), "a Windows host always has at least one drive");
    }
}
