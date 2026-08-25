//! The F9 pull-down menu state machine and item tables.
//!
//! The bar is a modal overlay over the panels rather than a `UiPhase`: it
//! sits alongside the phase in `State` so a file-op dialog and the menu can
//! never both claim the phase slot. Everything here is pure data plus index
//! arithmetic — dispatching an item's action is the caller's job
//! ([`crate::update`] turns a [`MenuAction`] back into a `Command`).

use crate::listing::SortMode;
use crate::panel::DisplayMode;
use crate::PanelSide;

/// The five menus, in bar order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuId {
    Left,
    Files,
    Commands,
    Options,
    Right,
}

pub const ALL_MENUS: [MenuId; 5] = [MenuId::Left, MenuId::Files, MenuId::Commands, MenuId::Options, MenuId::Right];

impl MenuId {
    pub fn title(self) -> &'static str {
        match self {
            MenuId::Left => "Left",
            MenuId::Files => "Files",
            MenuId::Commands => "Commands",
            MenuId::Options => "Options",
            MenuId::Right => "Right",
        }
    }

    /// The highlighted hotkey letter. All five are distinct, so a single
    /// keypress always identifies exactly one menu.
    pub fn hotkey(self) -> char {
        self.title().chars().next().expect("every menu title is non-empty")
    }

    pub fn index(self) -> usize {
        ALL_MENUS.iter().position(|m| *m == self).expect("ALL_MENUS covers every variant")
    }

    pub fn from_hotkey(c: char) -> Option<MenuId> {
        let c = c.to_ascii_lowercase();
        ALL_MENUS.iter().copied().find(|m| m.hotkey().to_ascii_lowercase() == c)
    }

    /// Horizontal traversal, wrapping from `Right` back to `Left`.
    pub fn next(self) -> MenuId {
        ALL_MENUS[(self.index() + 1) % ALL_MENUS.len()]
    }

    pub fn prev(self) -> MenuId {
        ALL_MENUS[(self.index() + ALL_MENUS.len() - 1) % ALL_MENUS.len()]
    }

    /// Which panel this menu's actions apply to. The `Left`/`Right` menus
    /// target their own side regardless of focus; every other menu acts on
    /// whichever panel is active.
    pub fn target_side(self, active: PanelSide) -> PanelSide {
        match self {
            MenuId::Left => PanelSide::Left,
            MenuId::Right => PanelSide::Right,
            _ => active,
        }
    }
}

/// What activating an item does. Items for features that land in later
/// milestones carry [`MenuAction::Unimplemented`] and render disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    ToggleInfoMode,
    /// Switch a panel's display mode directly (Brief/Full/Quick view). Tree
    /// keeps its own `EnterTree` action since entering it also kicks off
    /// the root's child-directory read (design D7).
    SetDisplayMode(DisplayMode),
    EnterTree,
    SortBy(SortMode),
    Reread,
    DriveSelect,
    Copy,
    Move,
    Mkdir,
    Delete,
    /// Files pull-down "Copy to clipboard" (Ctrl+C) — clipboard-export
    /// "Clipboard actions in menus".
    ClipboardFiles,
    /// Files pull-down "Copy path(s)" (Ctrl+Shift+Ins).
    ClipboardPaths,
    /// Files pull-down "Copy name(s)" — menu-only, no key binding.
    ClipboardNames,
    SelectGroup,
    DeselectGroup,
    InvertSelection,
    PanelsOnOff,
    FindFile,
    FuzzyJump,
    Quit,
    /// Options → Themes: opens the theme-selection picker dialog
    /// (theme-selection "Options menu opens the theme picker").
    OpenThemes,
    /// Rendered as a real entry so the menu matches its final shape, but
    /// greyed out and unselectable until its milestone lands.
    Unimplemented,
}

impl MenuAction {
    pub fn is_enabled(self) -> bool {
        self != MenuAction::Unimplemented
    }
}

/// One row of a pull-down: a selectable item or a `─` separator rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry {
    Separator,
    Item(MenuItem),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuItem {
    pub label: &'static str,
    /// Byte index into `label` of the highlighted hotkey letter.
    pub hotkey_index: usize,
    /// Right-aligned shortcut hint, e.g. `"Ctrl-L"`. Empty when the item
    /// has no key binding.
    pub shortcut: &'static str,
    pub action: MenuAction,
}

impl MenuItem {
    pub fn is_enabled(&self) -> bool {
        self.action.is_enabled()
    }

    pub fn hotkey(&self) -> Option<char> {
        self.label[self.hotkey_index..].chars().next()
    }
}

const fn item(label: &'static str, hotkey_index: usize, shortcut: &'static str, action: MenuAction) -> MenuEntry {
    MenuEntry::Item(MenuItem { label, hotkey_index, shortcut, action })
}

/// The `Left`/`Right` menus mirror each other exactly; only the panel they
/// act on differs, which [`MenuId::target_side`] resolves.
const PANEL_MENU: &[MenuEntry] = &[
    item("Brief", 0, "", MenuAction::SetDisplayMode(DisplayMode::Brief)),
    item("Full", 0, "", MenuAction::SetDisplayMode(DisplayMode::Full)),
    item("Tree", 0, "", MenuAction::EnterTree),
    item("Quick view", 0, "", MenuAction::SetDisplayMode(DisplayMode::QuickView)),
    item("Info", 0, "Ctrl-L", MenuAction::ToggleInfoMode),
    MenuEntry::Separator,
    item("Name", 0, "Ctrl-F3", MenuAction::SortBy(SortMode::Name)),
    item("Extension", 0, "Ctrl-F4", MenuAction::SortBy(SortMode::Extension)),
    item("Modif. time", 0, "Ctrl-F5", MenuAction::SortBy(SortMode::Time)),
    item("Size", 0, "Ctrl-F6", MenuAction::SortBy(SortMode::Size)),
    item("Unsorted", 0, "Ctrl-F7", MenuAction::SortBy(SortMode::Unsorted)),
    MenuEntry::Separator,
    item("Filter", 0, "", MenuAction::Unimplemented),
    item("Re-read", 0, "Ctrl-R", MenuAction::Reread),
    item("Drive select", 0, "Alt-F1/F2", MenuAction::DriveSelect),
    MenuEntry::Separator,
    item("New tab", 0, "Ctrl-T", MenuAction::Unimplemented),
    item("Close tab", 0, "Ctrl-W", MenuAction::Unimplemented),
];

const FILES_MENU: &[MenuEntry] = &[
    item("View", 0, "F3", MenuAction::Unimplemented),
    item("Edit", 0, "F4", MenuAction::Unimplemented),
    item("Copy", 0, "F5", MenuAction::Copy),
    item("Rename/Move", 0, "F6", MenuAction::Move),
    item("Make directory", 0, "F7", MenuAction::Mkdir),
    item("Delete", 0, "F8", MenuAction::Delete),
    MenuEntry::Separator,
    item("Copy to clipboard", 0, "Ctrl-C", MenuAction::ClipboardFiles),
    item("Copy path(s)", 0, "Ctrl-Sh-Ins", MenuAction::ClipboardPaths),
    item("Copy name(s)", 0, "", MenuAction::ClipboardNames),
    MenuEntry::Separator,
    item("Attributes", 0, "", MenuAction::Unimplemented),
    MenuEntry::Separator,
    item("Select group", 0, "+", MenuAction::SelectGroup),
    item("Deselect group", 0, "-", MenuAction::DeselectGroup),
    item("Invert selection", 0, "*", MenuAction::InvertSelection),
    MenuEntry::Separator,
    item("Quit", 0, "F10", MenuAction::Quit),
];

const COMMANDS_MENU: &[MenuEntry] = &[
    item("Find file", 0, "Alt-F7", MenuAction::FindFile),
    item("History", 0, "Alt-F8", MenuAction::Unimplemented),
    item("Swap panels", 0, "Ctrl-U", MenuAction::Unimplemented),
    item("Panels on/off", 0, "Ctrl-O", MenuAction::PanelsOnOff),
    item("Compare directories", 0, "", MenuAction::Unimplemented),
    item("Fuzzy jump", 0, "Ctrl-J", MenuAction::FuzzyJump),
    item("Menu file edit", 0, "", MenuAction::Unimplemented),
];

const OPTIONS_MENU: &[MenuEntry] = &[
    item("Configuration", 0, "", MenuAction::Unimplemented),
    item("Themes", 0, "", MenuAction::OpenThemes),
    item("Editor selection", 0, "", MenuAction::Unimplemented),
    MenuEntry::Separator,
    item("Save setup", 0, "", MenuAction::Unimplemented),
];

/// The pull-down contents for a menu.
pub fn entries(id: MenuId) -> &'static [MenuEntry] {
    match id {
        MenuId::Left | MenuId::Right => PANEL_MENU,
        MenuId::Files => FILES_MENU,
        MenuId::Commands => COMMANDS_MENU,
        MenuId::Options => OPTIONS_MENU,
    }
}

/// The open menu overlay. Absent from `State` entirely when the bar is
/// closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuState {
    pub active: MenuId,
    /// The bar can be open with every pull-down closed — that is the state
    /// Esc-from-a-pull-down leaves behind.
    pub pulldown_open: bool,
    pub selected: usize,
}

impl MenuState {
    /// F9: open the bar with the first menu active and its pull-down down.
    pub fn opened() -> MenuState {
        MenuState::for_menu(MenuId::Left)
    }

    /// Open `id`'s pull-down with the first selectable item highlighted.
    pub fn for_menu(id: MenuId) -> MenuState {
        MenuState { active: id, pulldown_open: true, selected: first_selectable(id).unwrap_or(0) }
    }

    pub fn items(&self) -> &'static [MenuEntry] {
        entries(self.active)
    }

    /// The item Enter would activate, or `None` when the pull-down is closed
    /// or the selection has landed on nothing selectable.
    pub fn selected_item(&self) -> Option<&'static MenuItem> {
        if !self.pulldown_open {
            return None;
        }
        match self.items().get(self.selected) {
            Some(MenuEntry::Item(item)) if item.is_enabled() => Some(item),
            _ => None,
        }
    }

    /// Move the selection by `delta` rows, landing only on enabled items and
    /// wrapping at the ends.
    pub fn move_selection(&mut self, delta: isize) {
        let entries = self.items();
        let len = entries.len();
        if len == 0 {
            return;
        }
        let step = if delta >= 0 { 1isize } else { -1isize };
        let mut index = self.selected as isize;
        // At most one full lap: if nothing is selectable we leave the
        // selection where it was rather than spinning.
        for _ in 0..len {
            index = (index + step).rem_euclid(len as isize);
            if is_selectable(&entries[index as usize]) {
                self.selected = index as usize;
                return;
            }
        }
    }

    /// Switch to an adjacent menu, keeping the pull-down open so horizontal
    /// traversal never shows an intermediate closed state.
    pub fn go_to(&mut self, id: MenuId) {
        self.active = id;
        self.pulldown_open = true;
        self.selected = first_selectable(id).unwrap_or(0);
    }
}

fn is_selectable(entry: &MenuEntry) -> bool {
    matches!(entry, MenuEntry::Item(item) if item.is_enabled())
}

/// The index of the first enabled item in `id`'s pull-down.
pub fn first_selectable(id: MenuId) -> Option<usize> {
    entries(id).iter().position(is_selectable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_activates_left_with_its_pulldown_down() {
        let menu = MenuState::opened();
        assert_eq!(menu.active, MenuId::Left);
        assert!(menu.pulldown_open);
        assert!(menu.selected_item().is_some());
    }

    #[test]
    fn bar_titles_are_the_five_menus_in_order() {
        let titles: Vec<&str> = ALL_MENUS.iter().map(|m| m.title()).collect();
        assert_eq!(titles, vec!["Left", "Files", "Commands", "Options", "Right"]);
    }

    #[test]
    fn hotkey_letters_are_unique_across_menus() {
        let mut keys: Vec<char> = ALL_MENUS.iter().map(|m| m.hotkey()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), ALL_MENUS.len(), "each menu must be reachable by a distinct hotkey");
    }

    #[test]
    fn hotkey_jump_resolves_case_insensitively() {
        assert_eq!(MenuId::from_hotkey('c'), Some(MenuId::Commands));
        assert_eq!(MenuId::from_hotkey('O'), Some(MenuId::Options));
        assert_eq!(MenuId::from_hotkey('z'), None);
    }

    #[test]
    fn horizontal_traversal_wraps_both_ways() {
        assert_eq!(MenuId::Right.next(), MenuId::Left);
        assert_eq!(MenuId::Left.prev(), MenuId::Right);
        assert_eq!(MenuId::Files.next(), MenuId::Commands);
    }

    #[test]
    fn vertical_selection_skips_separators_and_disabled_items() {
        let mut menu = MenuState::for_menu(MenuId::Files);
        // "Attributes" is disabled and bounded by separators on both sides,
        // so stepping past "Copy name(s)" must land on "Select group".
        let copy_names = entries(MenuId::Files)
            .iter()
            .position(|e| matches!(e, MenuEntry::Item(i) if i.label == "Copy name(s)"))
            .unwrap();
        menu.selected = copy_names;
        menu.move_selection(1);
        assert_eq!(menu.selected_item().map(|i| i.label), Some("Select group"));
    }

    #[test]
    fn vertical_selection_wraps_at_both_ends() {
        let mut menu = MenuState::for_menu(MenuId::Files);
        menu.move_selection(-1);
        assert_eq!(menu.selected_item().map(|i| i.label), Some("Quit"), "Up from the first item wraps to the last");
        menu.move_selection(1);
        assert_eq!(menu.selected_item().map(|i| i.label), Some("Copy"), "Down from the last wraps to the first");
    }

    #[test]
    fn selection_never_rests_on_a_disabled_item() {
        for id in ALL_MENUS {
            // An all-disabled menu (Options today) legitimately has nothing
            // to land on; `selected_item` returning `None` is what keeps
            // Enter from activating anything there.
            if first_selectable(id).is_none() {
                continue;
            }
            let mut menu = MenuState::for_menu(id);
            for delta in [1isize, -1] {
                for _ in 0..entries(id).len() * 2 {
                    menu.move_selection(delta);
                    match menu.items().get(menu.selected) {
                        Some(MenuEntry::Item(item)) => assert!(item.is_enabled(), "{:?} landed on disabled `{}`", id, item.label),
                        other => panic!("{id:?} selection landed on {other:?}, not an enabled item"),
                    }
                }
            }
        }
    }

    #[test]
    fn options_menu_themes_is_enabled_the_rest_still_render_disabled() {
        // Themes (theme-selection) is the first Options entry, and is the
        // canonical "renders disabled" example's *former* stand-in — see
        // `pulldown-menus` MODIFIED "Menu contents": the rest of the menu
        // stays a placeholder (pulldown-menus "Not-yet-available feature
        // renders disabled" now anchors on Attributes/Configuration/etc.
        // instead).
        let menu = MenuState::for_menu(MenuId::Options);
        assert_eq!(menu.selected_item().map(|i| i.label), Some("Themes"));
        for label in ["Configuration", "Editor selection", "Save setup"] {
            let item = entries(MenuId::Options)
                .iter()
                .find_map(|e| match e {
                    MenuEntry::Item(i) if i.label == label => Some(i),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("`{label}` missing from the Options menu"));
            assert!(!item.is_enabled(), "`{label}` should still render disabled");
        }
    }

    #[test]
    fn left_and_right_menus_mirror_each_other() {
        assert_eq!(entries(MenuId::Left), entries(MenuId::Right));
        assert_eq!(MenuId::Left.target_side(PanelSide::Right), PanelSide::Left);
        assert_eq!(MenuId::Right.target_side(PanelSide::Left), PanelSide::Right);
        assert_eq!(MenuId::Files.target_side(PanelSide::Right), PanelSide::Right, "non-panel menus follow focus");
    }

    #[test]
    fn files_menu_lists_its_specified_items() {
        let labels: Vec<&str> = entries(MenuId::Files)
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Item(i) => Some(i.label),
                MenuEntry::Separator => None,
            })
            .collect();
        assert_eq!(
            labels,
            vec![
                "View",
                "Edit",
                "Copy",
                "Rename/Move",
                "Make directory",
                "Delete",
                "Copy to clipboard",
                "Copy path(s)",
                "Copy name(s)",
                "Attributes",
                "Select group",
                "Deselect group",
                "Invert selection",
                "Quit",
            ]
        );
    }

    #[test]
    fn files_menu_clipboard_group_has_hyphenated_shortcut_hints() {
        // clipboard-export "Clipboard actions in menus": right-aligned
        // shortcut hints in the existing hyphenated style.
        let by_label = |label: &str| -> &'static MenuItem {
            entries(MenuId::Files)
                .iter()
                .find_map(|e| match e {
                    MenuEntry::Item(i) if i.label == label => Some(i),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("`{label}` missing from the Files menu"))
        };
        assert_eq!(by_label("Copy to clipboard").shortcut, "Ctrl-C");
        assert_eq!(by_label("Copy path(s)").shortcut, "Ctrl-Sh-Ins");
        assert_eq!(by_label("Copy name(s)").shortcut, "", "Names is menu-only: no key binding to hint");
    }

    #[test]
    fn panel_menus_cover_display_sort_filter_reread_drive_and_tabs() {
        let labels: Vec<&str> = entries(MenuId::Left)
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Item(i) => Some(i.label),
                MenuEntry::Separator => None,
            })
            .collect();
        for expected in ["Info", "Name", "Unsorted", "Filter", "Re-read", "Drive select", "New tab", "Close tab"] {
            assert!(labels.contains(&expected), "panel menu missing `{expected}`");
        }
    }

    #[test]
    fn not_yet_built_features_render_as_disabled_entries_not_omissions() {
        let history = entries(MenuId::Commands)
            .iter()
            .find_map(|e| match e {
                MenuEntry::Item(i) if i.label == "History" => Some(i),
                _ => None,
            })
            .expect("History is present even though it is unimplemented");
        assert!(!history.is_enabled());
    }

    #[test]
    fn find_file_and_fuzzy_jump_are_enabled_m5_entries() {
        for label in ["Find file", "Fuzzy jump"] {
            let item = entries(MenuId::Commands)
                .iter()
                .find_map(|e| match e {
                    MenuEntry::Item(i) if i.label == label => Some(i),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("`{label}` missing from the Commands menu"));
            assert!(item.is_enabled(), "`{label}` should be enabled now that M5 implements it");
        }
    }

    #[test]
    fn panel_menu_lists_the_display_mode_switches() {
        let labels: Vec<&str> = entries(MenuId::Left)
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Item(i) => Some(i.label),
                MenuEntry::Separator => None,
            })
            .collect();
        for expected in ["Brief", "Full", "Tree", "Quick view"] {
            assert!(labels.contains(&expected), "panel menu missing `{expected}`");
        }
    }

    #[test]
    fn go_to_keeps_the_pulldown_open_and_reselects() {
        let mut menu = MenuState::opened();
        menu.pulldown_open = false;
        menu.go_to(MenuId::Files);
        assert!(menu.pulldown_open);
        assert_eq!(menu.selected_item().map(|i| i.label), Some("Copy"));
    }

    #[test]
    fn selected_item_is_none_while_the_pulldown_is_closed() {
        let mut menu = MenuState::opened();
        menu.pulldown_open = false;
        assert!(menu.selected_item().is_none());
    }

    #[test]
    fn every_item_hotkey_index_points_inside_its_label() {
        for id in ALL_MENUS {
            for entry in entries(id) {
                if let MenuEntry::Item(item) = entry {
                    assert!(item.hotkey().is_some(), "`{}` has an out-of-range hotkey index", item.label);
                }
            }
        }
    }
}
