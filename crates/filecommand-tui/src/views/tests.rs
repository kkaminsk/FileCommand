//! Geometry agreement between what each dialog and menu draws and what the
//! hit map records for it (reducer-overlay-structure design D8; mouse-input
//! "Click targets match the rendered geometry", "Geometry agrees at every
//! size").

use std::ffi::OsString;
use std::path::PathBuf;

use filecommand_core::dialogs::FileActionMenuState;
use filecommand_core::fs_ops::dialog::{DropButtons, FileOpSetup, RunningDialog};
use filecommand_core::fs_ops::{ConflictInfo, ErrorInfo, JobKind, ProgressInfo, SkippedItem, SourceItem};
use filecommand_core::line_edit::LineEdit;
use filecommand_core::listing::{display_width, truncate_with_ellipsis};
use filecommand_core::menu::{entries, MenuEntry, MenuId, MenuState};
use filecommand_core::theme::Theme;
use filecommand_core::update::ButtonId;
use filecommand_core::{Overlay, State, UiPhase};
use proptest::prelude::*;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::*;

fn identity() -> [String; 4] {
    ["FileCommand".to_string(), "v0".to_string(), "test".to_string(), "fixture".to_string()]
}

/// Render `state` at `size` and return the painted buffer with the frame's
/// hit map.
fn frame(state: &State, size: (u16, u16)) -> (Buffer, HitMap) {
    let area = Rect { x: 0, y: 0, width: size.0, height: size.1 };
    let mut buf = Buffer::empty(area);
    let out = render(&mut buf, area, state, ColorDepth::Ansi16, &identity(), "3:04 PM", None);
    (buf, out.hitmap)
}

/// The text painted on the cells of `rect`, row by row.
fn cells(buf: &Buffer, rect: Rect) -> String {
    let mut out = String::new();
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            out.push_str(buf[(x, y)].symbol());
        }
    }
    out
}

/// What a button's cells must spell: the label the renderer paints for it.
fn label_of(id: ButtonId) -> &'static str {
    match id {
        ButtonId::DeleteConfirmYes => "(Y)es",
        ButtonId::DeleteConfirmNo => "(N)o",
        ButtonId::ConflictOverwrite => "(O)verwrite",
        ButtonId::ConflictOverwriteAll => "Over(w)rite All",
        ButtonId::ConflictSkip => "(S)kip",
        ButtonId::ConflictSkipAll => "Skip (A)ll",
        ButtonId::ConflictRename => "(R)ename",
        ButtonId::ConflictCancel => "(C)ancel",
        ButtonId::ErrorRetry => "(R)etry",
        ButtonId::ErrorSkip => "(S)kip",
        ButtonId::ErrorSkipAll => "Skip (A)ll",
        ButtonId::ErrorAbort => "A(b)ort",
        ButtonId::ProgressCancel => "[ Cancel ]",
        ButtonId::SummaryContinue => "Press any key to continue",
        ButtonId::QuitYes => "Y",
        ButtonId::QuitNo => "N",
        ButtonId::DropDialogCopy => "[ Copy ]",
        ButtonId::DropDialogMove => "[ Move ]",
        ButtonId::DropDialogCancel => "[ Cancel ]",
    }
}

fn state_at(size: (u16, u16), phase: UiPhase) -> State {
    let mut state = State::empty(Theme::classic());
    state.term_size = size;
    state.phase = phase;
    state
}

fn source(name: &str, is_dir: bool) -> SourceItem {
    SourceItem { original_name: OsString::from(name), path: PathBuf::from("/left").join(name), is_dir }
}

/// Every hit rectangle the frame records lies inside the screen and spells
/// exactly its button's label (the summary footer is one whole row, so its
/// cells spell the clamped footer text and nothing else).
fn assert_buttons_match_the_painted_labels(buf: &Buffer, hm: &HitMap, size: (u16, u16)) -> Result<(), TestCaseError> {
    for (rect, id) in &hm.dialog_buttons {
        prop_assert!(rect.height == 1 && rect.width > 0, "{id:?} at {rect:?}");
        prop_assert!(rect.right() <= size.0 && rect.bottom() <= size.1, "{id:?} at {rect:?} is off a {size:?} screen");
        let painted = cells(buf, *rect);
        let label = label_of(*id);
        if *id == ButtonId::SummaryContinue {
            let clamped = truncate_with_ellipsis(label, rect.width as usize);
            prop_assert_eq!(painted.trim_end(), clamped.as_str(), "the footer row");
        } else {
            prop_assert_eq!(painted.as_str(), label, "{:?} at {:?}", id, rect);
            prop_assert_eq!(rect.width as usize, display_width(label));
        }
    }
    Ok(())
}

/// Every row rectangle lies inside the screen, is one row tall, and spells
/// the row the renderer painted for that entry (its label, or the clamped
/// `…` form of it).
fn assert_rows_match_the_painted_rows(buf: &Buffer, rows: &[(Rect, usize)], labels: &[Option<&str>], size: (u16, u16)) -> Result<(), TestCaseError> {
    for (rect, index) in rows {
        prop_assert_eq!(rect.height, 1);
        prop_assert!(rect.right() <= size.0 && rect.bottom() <= size.1, "row {index} at {rect:?} is off a {size:?} screen");
        let label = labels.get(*index).copied().flatten().unwrap_or_else(|| panic!("row {index} names an item"));
        let painted = cells(buf, *rect);
        prop_assert!(painted.contains(label) || painted.contains('\u{2026}'), "row {} at {:?} paints {:?}, not {:?}", index, rect, painted, label);
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn delete_confirm_buttons_are_exactly_their_painted_labels(w in 60u16..=200, h in 16u16..=60, name_len in 1usize..=200, second in any::<bool>(), hidden in 0usize..=2) {
        let name = "n".repeat(name_len);
        let setup = FileOpSetup::DeleteConfirm {
            sources: vec![source(&name, second), source("b.txt", false), source("c.txt", false)],
            source_dir: PathBuf::from("/left"),
            needs_second_confirm: second,
            confirmed_once: second,
            hidden_by_filter: hidden,
        };
        let state = state_at((w, h), UiPhase::FileOpSetup(setup));
        let (buf, hm) = frame(&state, (w, h));
        prop_assert_eq!(hm.dialog_buttons.len(), 2, "both buttons are recorded at every size");
        assert_buttons_match_the_painted_labels(&buf, &hm, (w, h))?;
    }

    #[test]
    fn drop_dialog_buttons_are_exactly_their_painted_labels(w in 60u16..=200, h in 16u16..=60, focused_move in any::<bool>(), hidden in 0usize..=2, text_len in 0usize..=120) {
        let setup = FileOpSetup::DestinationInput {
            kind: JobKind::Copy,
            sources: vec![source("a.txt", false), source("b.txt", false)],
            source_dir: PathBuf::from("/left"),
            input: LineEdit::from("x".repeat(text_len)),
            buttons: Some(DropButtons { focused: if focused_move { JobKind::Move } else { JobKind::Copy } }),
            hidden_by_filter: hidden,
        };
        let state = state_at((w, h), UiPhase::FileOpSetup(setup));
        let (buf, hm) = frame(&state, (w, h));
        prop_assert_eq!(hm.dialog_buttons.len(), 3);
        assert_buttons_match_the_painted_labels(&buf, &hm, (w, h))?;
    }

    #[test]
    fn quit_dialog_hotkeys_are_exactly_their_painted_cells(w in 60u16..=200, h in 16u16..=60, over_menu in any::<bool>()) {
        let mut state = state_at((w, h), UiPhase::Panels);
        if over_menu {
            state.open_overlay(Overlay::Menu(MenuState::for_menu(MenuId::Files)));
        }
        state.quit_confirm = true;
        let (buf, hm) = frame(&state, (w, h));
        prop_assert_eq!(hm.dialog_buttons.len(), 2);
        assert_buttons_match_the_painted_labels(&buf, &hm, (w, h))?;
    }

    #[test]
    fn progress_cancel_is_exactly_its_painted_label(w in 60u16..=200, h in 16u16..=60, done in 0u64..=100, name_len in 0usize..=120) {
        let progress = ProgressInfo { files_done: 1, files_total: 9, bytes_done: done, bytes_total: 100, current_file: OsString::from("f".repeat(name_len)) };
        let phase = UiPhase::FileOpRunning { source_dir: PathBuf::from("/left"), dest_dir: PathBuf::from("/right"), dialog: RunningDialog::Progress { kind: JobKind::Copy, progress } };
        let state = state_at((w, h), phase);
        let (buf, hm) = frame(&state, (w, h));
        prop_assert_eq!(hm.dialog_buttons.len(), 1);
        assert_buttons_match_the_painted_labels(&buf, &hm, (w, h))?;
    }

    #[test]
    fn conflict_dialog_hotkeys_are_exactly_their_painted_labels(w in 60u16..=200, h in 16u16..=60, renaming in any::<bool>(), name_len in 1usize..=120) {
        let info = ConflictInfo {
            source_name: OsString::from("s".repeat(name_len)),
            source_size: 1234,
            source_modified: None,
            target_path: PathBuf::from("/right/t.txt"),
            target_size: 99,
            target_modified: None,
        };
        let rename_input = renaming.then(|| LineEdit::from("new name"));
        let phase = UiPhase::FileOpRunning {
            source_dir: PathBuf::from("/left"),
            dest_dir: PathBuf::from("/right"),
            dialog: RunningDialog::Conflict { kind: JobKind::Copy, info, progress: ProgressInfo::starting(3, 30), rename_input },
        };
        let state = state_at((w, h), phase);
        let (buf, hm) = frame(&state, (w, h));
        if renaming {
            prop_assert!(hm.dialog_buttons.is_empty(), "no hotkey rows while the rename field is open");
        } else {
            prop_assert!(hm.dialog_buttons.iter().any(|(_, id)| *id == ButtonId::ConflictCancel), "the Cancel row is always whole");
        }
        assert_buttons_match_the_painted_labels(&buf, &hm, (w, h))?;
    }

    #[test]
    fn error_dialog_hotkeys_are_exactly_their_painted_labels(w in 60u16..=200, h in 16u16..=60, msg_len in 1usize..=160, path_len in 1usize..=160) {
        let info = ErrorInfo { path: PathBuf::from("p".repeat(path_len)), message: "m".repeat(msg_len) };
        let phase = UiPhase::FileOpRunning {
            source_dir: PathBuf::from("/left"),
            dest_dir: PathBuf::from("/right"),
            dialog: RunningDialog::Error { kind: JobKind::Move, info, progress: ProgressInfo::starting(3, 30) },
        };
        let state = state_at((w, h), phase);
        let (buf, hm) = frame(&state, (w, h));
        prop_assert!(!hm.dialog_buttons.is_empty());
        assert_buttons_match_the_painted_labels(&buf, &hm, (w, h))?;
    }

    #[test]
    fn skipped_summary_footer_is_exactly_its_painted_row(w in 60u16..=200, h in 16u16..=60, skipped in 0usize..=10, warnings in 0usize..=10) {
        let mut items: Vec<SkippedItem> = (0..skipped).map(|i| SkippedItem::skipped(PathBuf::from(format!("/left/s{i}.txt")), "exists".to_string())).collect();
        items.extend((0..warnings).map(|i| SkippedItem::warning(PathBuf::from(format!("/left/w{i}.txt")), "timestamp".to_string())));
        if items.is_empty() {
            items.push(SkippedItem::skipped(PathBuf::from("/left/only.txt"), "exists".to_string()));
        }
        let state = state_at((w, h), UiPhase::FileOpSummary(items));
        let (buf, hm) = frame(&state, (w, h));
        prop_assert_eq!(hm.dialog_buttons.len(), 1);
        assert_buttons_match_the_painted_labels(&buf, &hm, (w, h))?;
    }

    #[test]
    fn file_action_menu_rows_are_exactly_the_painted_rows(w in 60u16..=200, h in 16u16..=60, executable in any::<bool>(), is_dir in any::<bool>(), name_len in 1usize..=80) {
        let menu = FileActionMenuState::open(OsString::from("f".repeat(name_len)), is_dir, executable && !is_dir, false);
        let labels: Vec<Option<&str>> = menu.entries.iter().map(|e| Some(e.label())).collect();
        let mut state = state_at((w, h), UiPhase::Panels);
        state.open_overlay(Overlay::FileActionMenu(menu));
        let (buf, hm) = frame(&state, (w, h));
        prop_assert_eq!(hm.file_action_menu_items.len(), labels.len(), "every entry has a row at every size from the floor up");
        assert_rows_match_the_painted_rows(&buf, &hm.file_action_menu_items, &labels, (w, h))?;
    }

    #[test]
    fn pulldown_rows_and_titles_are_exactly_the_painted_cells(w in 60u16..=200, h in 16u16..=60, which in 0usize..5) {
        let id = filecommand_core::menu::ALL_MENUS[which];
        let labels: Vec<Option<&str>> = entries(id).iter().map(|e| match e { MenuEntry::Item(item) => Some(item.label), MenuEntry::Separator => None }).collect();
        let mut state = state_at((w, h), UiPhase::Panels);
        state.open_overlay(Overlay::Menu(MenuState::for_menu(id)));
        let (buf, hm) = frame(&state, (w, h));
        // A short terminal clamps the pull-down (responsive-layout "Unified
        // overlay geometry"), so only the rows that fit are drawn — and only
        // those are recorded.
        let items = labels.iter().filter(|l| l.is_some()).count();
        prop_assert!(!hm.menu_items.is_empty() && hm.menu_items.len() <= items, "{} rows recorded for {} items", hm.menu_items.len(), items);
        assert_rows_match_the_painted_rows(&buf, &hm.menu_items, &labels, (w, h))?;
        for (rect, title) in &hm.menu_titles {
            prop_assert_eq!(rect.y, 0);
            let painted = cells(&buf, *rect);
            prop_assert_eq!(painted.as_str(), title.title(), "the title's cells");
        }
        prop_assert_eq!(hm.menu_titles.len(), 5);
    }
}
