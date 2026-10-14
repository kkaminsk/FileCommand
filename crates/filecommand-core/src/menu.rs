//! The F9 pull-down menu state machine and item tables.
//!
//! The bar is a modal overlay over the panels rather than a `UiPhase`: it
//! sits alongside the phase in `State` so a file-op dialog and the menu can
//! never both claim the phase slot. Everything here is pure data plus index
//! arithmetic — dispatching an item's action is the caller's job
//! ([`crate::update`](fn@crate::update) turns a [`MenuAction`] back into a `Command`).

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

/// What activating an item does. Every shipped item is wired
/// (pulldown-menu-completion); [`MenuAction::Unimplemented`] is retained for
/// future proposals, whose items render disabled until they land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    ToggleInfoMode,
    /// Switch a panel's display mode directly (Brief/Full/Quick view). Tree
    /// keeps its own `EnterTree` action since entering it also kicks off
    /// the root's child-directory read (design D7).
    SetDisplayMode(DisplayMode),
    EnterTree,
    SortBy(SortMode),
    /// Left/Right → Filter: start the quick filter on the menu's own panel
    /// (which becomes the active panel so typing reaches it — design D1).
    QuickFilter,
    Reread,
    DriveSelect,
    /// Left/Right → New tab (Ctrl+T) on the menu's own panel.
    NewTab,
    /// Left/Right → Close tab (Ctrl+W) on the menu's own panel.
    CloseTab,
    /// Files → View (F3).
    View,
    /// Files → Edit (F4).
    Edit,
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
    /// Commands → History (Alt+F8): open the command-history dialog
    /// (command-line "Command history dialog (Alt+F8)").
    History,
    /// Commands → Swap panels (Ctrl+U): exchange the two panels' state
    /// (panel-navigation "Swap panels (Ctrl+U)").
    SwapPanels,
    FuzzyJump,
    /// Commands → Menu file edit: open `usermenu.toml` in the F4 editor path
    /// (user-menu "Edit the user menu file from the Commands menu").
    EditMenuFile,
    Quit,
    /// Options → Themes: opens the theme-selection picker dialog
    /// (theme-selection "Options menu opens the theme picker").
    OpenThemes,
    /// Rendered as a real entry so the menu matches its final shape, but
    /// greyed out and unselectable until its milestone lands. No shipped item
    /// uses it (pulldown-menu-completion design D5).
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
    item("Filter", 0, "Ctrl-P", MenuAction::QuickFilter),
    item("Re-read", 0, "Ctrl-R", MenuAction::Reread),
    item("Drive select", 0, "Alt-F1/F2", MenuAction::DriveSelect),
    MenuEntry::Separator,
    item("New tab", 0, "Ctrl-T", MenuAction::NewTab),
    item("Close tab", 0, "Ctrl-W", MenuAction::CloseTab),
];

const FILES_MENU: &[MenuEntry] = &[
    item("View", 0, "F3", MenuAction::View),
    item("Edit", 0, "F4", MenuAction::Edit),
    item("Copy", 0, "F5", MenuAction::Copy),
    item("Rename/Move", 0, "F6", MenuAction::Move),
    item("Make directory", 0, "F7", MenuAction::Mkdir),
    item("Delete", 0, "F8", MenuAction::Delete),
    MenuEntry::Separator,
    item("Copy to clipboard", 0, "Ctrl-C", MenuAction::ClipboardFiles),
    item("Copy path(s)", 0, "Ctrl-Sh-Ins", MenuAction::ClipboardPaths),
    item("Copy name(s)", 0, "", MenuAction::ClipboardNames),
    MenuEntry::Separator,
    item("Select group", 0, "+", MenuAction::SelectGroup),
    item("Deselect group", 0, "-", MenuAction::DeselectGroup),
    item("Invert selection", 0, "*", MenuAction::InvertSelection),
    MenuEntry::Separator,
    item("Quit", 0, "F10", MenuAction::Quit),
];

const COMMANDS_MENU: &[MenuEntry] = &[
    item("Find file", 0, "Alt-F7", MenuAction::FindFile),
    item("History", 0, "Alt-F8", MenuAction::History),
    item("Swap panels", 0, "Ctrl-U", MenuAction::SwapPanels),
    item("Panels on/off", 0, "Ctrl-O", MenuAction::PanelsOnOff),
    item("Fuzzy jump", 0, "Ctrl-J", MenuAction::FuzzyJump),
    item("Menu file edit", 0, "", MenuAction::EditMenuFile),
];

const OPTIONS_MENU: &[MenuEntry] = &[item("Themes", 0, "", MenuAction::OpenThemes)];

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
    /// `State::clock_ms` when the bar opened. Stamped by the reducer (the
    /// constructors below leave it `0`), read only by the renderer to drive
    /// the bar's entrance sweep (ui-animations "Render-only motion model
    /// driven by the reducer clock").
    pub opened_at_ms: u64,
    /// `State::clock_ms` when the current pull-down began unfolding —
    /// re-stamped on Left/Right traversal and hotkey jumps, since the new
    /// pull-down unfolds while already open and interactive (design D4).
    pub pulldown_opened_at_ms: u64,
    /// `State::clock_ms` when the highlight last moved within the current
    /// pull-down, or `None` if it has not moved since the pull-down opened
    /// (opening is the unfold's job, not a highlight flash).
    pub selected_at_ms: Option<u64>,
}

impl MenuState {
    /// F9: open the bar with the first menu active and its pull-down down.
    pub fn opened() -> MenuState {
        MenuState::for_menu(MenuId::Left)
    }

    /// Open `id`'s pull-down with the first selectable item highlighted.
    pub fn for_menu(id: MenuId) -> MenuState {
        MenuState {
            active: id,
            pulldown_open: true,
            selected: first_selectable(id).unwrap_or(0),
            opened_at_ms: 0,
            pulldown_opened_at_ms: 0,
            selected_at_ms: None,
        }
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
        self.selected = move_selection_in(self.items(), self.selected, delta);
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

/// The selection-walk core of [`MenuState::move_selection`], factored out so
/// the skip-disabled/separator rule can be tested against a local fixture
/// rather than relying on shipped items being disabled (pulldown-menu-
/// completion design D5: no shipped item is disabled).
fn move_selection_in(entries: &[MenuEntry], selected: usize, delta: isize) -> usize {
    let len = entries.len();
    if len == 0 {
        return selected;
    }
    let step = if delta >= 0 { 1isize } else { -1isize };
    let mut index = selected as isize;
    // At most one full lap: if nothing is selectable we leave the
    // selection where it was rather than spinning.
    for _ in 0..len {
        index = (index + step).rem_euclid(len as isize);
        if is_selectable(&entries[index as usize]) {
            return index as usize;
        }
    }
    selected
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
    fn vertical_selection_skips_separators() {
        let mut menu = MenuState::for_menu(MenuId::Files);
        // A separator sits between "Copy name(s)" and "Select group", so
        // stepping down from the former must land on the latter.
        let copy_names = entries(MenuId::Files).iter().position(|e| matches!(e, MenuEntry::Item(i) if i.label == "Copy name(s)")).unwrap();
        menu.selected = copy_names;
        menu.move_selection(1);
        assert_eq!(menu.selected_item().map(|i| i.label), Some("Select group"));
    }

    #[test]
    fn vertical_selection_skips_disabled_items_in_a_local_fixture() {
        // No shipped item is disabled (design D5), so the skip-disabled rule
        // is pinned against a local fixture: a disabled item bounded by
        // separators must never be landed on, from either direction.
        let fixture: Vec<MenuEntry> = vec![
            item("Alpha", 0, "", MenuAction::Copy),
            MenuEntry::Separator,
            item("Beta", 0, "", MenuAction::Unimplemented),
            MenuEntry::Separator,
            item("Gamma", 0, "", MenuAction::Quit),
        ];
        assert!(!MenuAction::Unimplemented.is_enabled(), "sanity: the fixture's disabled item is disabled");
        assert_eq!(move_selection_in(&fixture, 0, 1), 4, "Down from Alpha skips the disabled Beta and both separators");
        assert_eq!(move_selection_in(&fixture, 4, -1), 0, "Up from Gamma skips them going back");
    }

    #[test]
    fn vertical_selection_wraps_at_both_ends() {
        let mut menu = MenuState::for_menu(MenuId::Files);
        menu.move_selection(-1);
        assert_eq!(menu.selected_item().map(|i| i.label), Some("Quit"), "Up from the first item wraps to the last");
        menu.move_selection(1);
        assert_eq!(menu.selected_item().map(|i| i.label), Some("View"), "Down from the last wraps to the first");
    }

    #[test]
    fn selection_never_rests_on_a_disabled_item() {
        for id in ALL_MENUS {
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
    fn every_shipped_item_is_enabled() {
        // pulldown-menu-completion "Every listed item is wired": no shipped
        // menu carries a greyed-out placeholder any more (design D5 — the
        // disabled-rendering rule stays available for future items).
        for id in ALL_MENUS {
            for entry in entries(id) {
                if let MenuEntry::Item(item) = entry {
                    assert!(item.is_enabled(), "{:?} `{}` must not ship disabled", id, item.label);
                }
            }
        }
    }

    #[test]
    fn options_menu_lists_themes_alone() {
        // Options collapsed to its one wired entry — no separator (design
        // D5): Configuration, Editor selection, and Save setup were removed.
        let entries = entries(MenuId::Options);
        assert_eq!(entries.len(), 1, "Options lists Themes alone");
        assert!(matches!(entries[0], MenuEntry::Item(i) if i.label == "Themes" && i.action == MenuAction::OpenThemes));
        let menu = MenuState::for_menu(MenuId::Options);
        assert_eq!(menu.selected_item().map(|i| i.label), Some("Themes"));
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
    fn panel_menus_list_their_specified_items_in_order() {
        // pulldown-menus "Menu contents": the full Left/Right pull-down, in
        // order (`Left and Right menus mirror each other`).
        let labels: Vec<&str> = entries(MenuId::Left)
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Item(i) => Some(i.label),
                MenuEntry::Separator => None,
            })
            .collect();
        assert_eq!(
            labels,
            vec![
                "Brief",
                "Full",
                "Tree",
                "Quick view",
                "Info",
                "Name",
                "Extension",
                "Modif. time",
                "Size",
                "Unsorted",
                "Filter",
                "Re-read",
                "Drive select",
                "New tab",
                "Close tab",
            ]
        );
    }

    #[test]
    fn commands_menu_lists_its_specified_items_in_order() {
        // pulldown-menus "Menu contents": Commands, in order — Compare
        // directories is gone, History/Swap panels/Menu file edit are wired.
        let labels: Vec<&str> = entries(MenuId::Commands)
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Item(i) => Some(i.label),
                MenuEntry::Separator => None,
            })
            .collect();
        assert_eq!(labels, vec!["Find file", "History", "Swap panels", "Panels on/off", "Fuzzy jump", "Menu file edit"]);
    }

    #[test]
    fn removed_items_are_absent_from_every_menu() {
        // pulldown-menus "Removed items are absent" (design D5: remove,
        // don't hide).
        for id in ALL_MENUS {
            for entry in entries(id) {
                if let MenuEntry::Item(item) = entry {
                    for removed in ["Attributes", "Compare directories", "Configuration", "Editor selection", "Save setup"] {
                        assert_ne!(item.label, removed, "{id:?} still lists removed `{removed}`");
                    }
                }
            }
        }
    }

    #[test]
    fn newly_wired_items_carry_their_keyboard_equivalents_shortcut_hints() {
        // The wired items show the same hyphenated hint style as the rest of
        // the menu (F3/F4/Ctrl+P/Ctrl+T/Ctrl+W/Alt+F8/Ctrl+U bindings).
        let by_label = |id: MenuId, label: &str| -> &'static MenuItem {
            entries(id)
                .iter()
                .find_map(|e| match e {
                    MenuEntry::Item(i) if i.label == label => Some(i),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("`{label}` missing from the {id:?} menu"))
        };
        assert_eq!(by_label(MenuId::Files, "View").shortcut, "F3");
        assert_eq!(by_label(MenuId::Files, "Edit").shortcut, "F4");
        assert_eq!(by_label(MenuId::Left, "Filter").shortcut, "Ctrl-P");
        assert_eq!(by_label(MenuId::Left, "New tab").shortcut, "Ctrl-T");
        assert_eq!(by_label(MenuId::Left, "Close tab").shortcut, "Ctrl-W");
        assert_eq!(by_label(MenuId::Commands, "History").shortcut, "Alt-F8");
        assert_eq!(by_label(MenuId::Commands, "Swap panels").shortcut, "Ctrl-U");
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
        assert_eq!(menu.selected_item().map(|i| i.label), Some("View"));
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
