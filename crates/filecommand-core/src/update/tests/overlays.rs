use super::*;

#[test]
fn file_action_menu_does_not_consume_or_alter_the_selection() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries =
        vec![file_entry("a.txt", 1), file_entry("report.txt", 2), file_entry("c.txt", 3), file_entry("d.txt", 4), file_entry("e.txt", 5)];
    for n in ["a.txt", "report.txt", "c.txt", "d.txt", "e.txt"] {
        state.left.selected.insert(OsString::from(n));
    }
    state.left.cursor = 1; // report.txt
    let selected_before = state.left.selected.clone();

    let (state, _) = update(state, Command::Enter);
    let menu = state.file_action_menu().expect("menu opens");
    assert_eq!(menu.target_name, OsString::from("report.txt"), "the menu targets only the cursor entry");
    assert_eq!(state.left.selected, selected_before, "opening the menu does not touch the selection");
    assert_eq!(state.left.selected.len(), 5);

    let (state, _) = update(state, Command::FileActionMenuCancel);
    assert_eq!(state.left.selected, selected_before, "closing the menu leaves the selection intact");
}

#[test]
fn file_action_menu_does_not_open_when_the_command_buffer_is_non_empty() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\NORTON");
    state.left.entries = vec![file_entry("readme.txt", 1)];
    let state = type_line(state, "cd nosuchdir");
    let (state, effects) = update(state, Command::Enter);
    assert!(state.file_action_menu().is_none(), "a non-empty command buffer takes precedence over the menu");
    // The unlisted target is checked asynchronously (offload-blocking-io):
    // the Enter itself spawns nothing and starts no listing.
    assert!(
        matches!(effects.as_slice(), [Effect::ProbePath { .. }]),
        "the cd only probes its target — it spawns and starts nothing, got {effects:?}"
    );
    assert_eq!(state.left.cwd, PathBuf::from(r"C:\NORTON"), "the pending cd leaves the panel untouched");
}

#[test]
fn file_action_menu_does_not_open_for_a_directory_which_still_navigates() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    let (state, effects) = update(state, Command::Enter);
    assert!(state.file_action_menu().is_none(), "Enter on a directory must not open the menu");
    assert_eq!(without_git_info_effects(effects), vec![start_listing(&state, PanelSide::Left, "/left/sub")]);
}

#[test]
fn file_action_menu_up_down_moves_the_highlight_clamped_at_both_ends() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    // Non-executable: Open, View, Edit, Send to clipboard, Copy, Rename,
    // Move, Delete (8 entries).
    let (state, _) = update(state, Command::FileActionMenuMove(-5));
    assert_eq!(state.file_action_menu().unwrap().cursor, 0, "Up from the first entry holds, it does not wrap");
    let (state, _) = update(state, Command::FileActionMenuMove(100));
    let menu = state.file_action_menu().unwrap();
    assert_eq!(menu.cursor, menu.entries.len() - 1, "Down past the last entry clamps, it does not wrap");
    assert_eq!(menu.selected(), FileActionMenuEntry::Delete);
}

#[test]
fn file_action_menu_esc_closes_with_no_action_and_leaves_the_cursor_and_panel_untouched() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let (state, effects) = update(state, Command::FileActionMenuCancel);
    assert!(state.file_action_menu().is_none());
    assert!(effects.is_empty());
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(state.left.cursor, 0);
}

#[test]
fn file_action_menu_first_letter_hotkey_activates_directly() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let (state, _) = update(state, Command::FileActionMenuHotkey('D'));
    assert!(state.file_action_menu().is_none(), "the hotkey closes the menu");
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { sources, .. }) => {
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].original_name, OsString::from("notes.txt"));
        }
        other => panic!("expected the Delete hotkey to open DeleteConfirm, got {other:?}"),
    }
}

#[test]
fn file_action_menu_unknown_hotkey_is_a_no_op() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let (state, effects) = update(state, Command::FileActionMenuHotkey('Z'));
    assert!(state.file_action_menu().is_some(), "an unmatched hotkey leaves the menu open");
    assert!(effects.is_empty());
}

#[test]
fn file_action_menu_targets_the_entry_it_opened_on_even_if_the_cursor_drifts_from_a_background_listing_refresh() {
    // Regression: ListingChunk is applied unconditionally regardless of any
    // open modal (including this menu), and insert_streamed re-pins the
    // cursor to row 0 whenever cursor_user_moved is still false — the
    // normal state right after Enter opened the menu. The menu must keep
    // acting on the entry it was opened on (`b.txt`), never on whatever a
    // background listing update lands under the cursor in the meantime.
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("b.txt", 1)];
    state.left.cursor = 0;
    let (state, _) = update(state, Command::Enter); // opens the menu on b.txt
    let menu = state.file_action_menu().unwrap();
    assert_eq!(menu.target_name, OsString::from("b.txt"));

    // A background listing chunk inserts a.txt, which sorts ahead of
    // b.txt, and re-pins the (not-yet-user-moved) cursor to row 0.
    let (state, _) = listing_chunk(state, PanelSide::Left, vec![file_entry("a.txt", 1)]);
    assert_eq!(state.left.cursor, 0, "setup precondition: the cursor drifted onto a.txt");
    assert_eq!(state.left.entries[state.left.cursor].name, OsString::from("a.txt"));

    let (state, _) = update(state, Command::FileActionMenuHotkey('D'));
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { sources, .. }) => {
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].original_name, OsString::from("b.txt"), "must target the entry the menu opened on, not the drifted cursor");
        }
        other => panic!("expected DeleteConfirm targeting b.txt (the menu's original target), got {other:?}"),
    }
}

#[test]
fn file_action_menu_view_routes_to_the_f3_viewer_path() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let (state, _) = update(state, Command::FileActionMenuMove(1)); // Open is highlighted first; one Down lands on View
    let (state, effects) = update(state, Command::FileActionMenuConfirm);
    assert!(state.file_action_menu().is_none());
    assert_eq!(effects, vec![Effect::OpenViewer { request: 3, path: PathBuf::from("/left/notes.txt") }]);
    assert_eq!(state.phase, UiPhase::Panels, "opening is I/O; the phase flips only once ViewerOpened comes back");
}

#[test]
fn file_action_menu_edit_routes_to_the_f4_edit_path() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let (state, _) = update(state, Command::FileActionMenuMove(2)); // Open, View, Edit
    let (state, effects) = update(state, Command::FileActionMenuConfirm);
    assert!(state.file_action_menu().is_none());
    assert_eq!(effects, vec![Effect::OpenEditor { request: 3, path: PathBuf::from("/left/notes.txt") }]);
}

#[test]
fn file_action_menu_copy_opens_the_f5_destination_dialog_scoped_to_the_target_only() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1), file_entry("report.txt", 2)];
    state.left.selected.insert(OsString::from("a.txt")); // an unrelated selection must not leak in
    state.left.cursor = 1; // report.txt
    let (state, _) = update(state, Command::Enter);
    let (state, _) = update(state, Command::FileActionMenuMove(4)); // Open, View, Edit, SendToClipboard, Copy
    let (state, effects) = update(state, Command::FileActionMenuConfirm);
    assert!(effects.is_empty(), "opening the destination dialog is not itself a mutation");
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, input, .. }) => {
            assert_eq!(kind, JobKind::Copy);
            assert_eq!(sources.len(), 1, "scoped to the menu's target only, not the selection");
            assert_eq!(sources[0].original_name, OsString::from("report.txt"));
            assert_eq!(input.text(), PathBuf::from("/right").display().to_string(), "pre-filled with the opposite panel's path");
        }
        other => panic!("expected FileOpSetup::DestinationInput, got {other:?}"),
    }
}

#[test]
fn file_action_menu_move_opens_the_f6_destination_dialog_scoped_to_the_target_only() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let (state, _) = update(state, Command::FileActionMenuMove(6)); // Open, View, Edit, SendToClipboard, Copy, Rename, Move
    let (state, _) = update(state, Command::FileActionMenuConfirm);
    match state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind, sources, .. }) => {
            assert_eq!(kind, JobKind::Move);
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].original_name, OsString::from("notes.txt"));
        }
        other => panic!("expected FileOpSetup::DestinationInput, got {other:?}"),
    }
}

#[test]
fn file_action_menu_delete_requires_the_existing_confirmation_and_declining_deletes_nothing() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let (state, _) = update(state, Command::FileActionMenuHotkey('D'));
    let (state, effects) = update(state, Command::FileOpCancel);
    assert_eq!(state.phase, UiPhase::Panels, "declining returns to the panels");
    assert!(effects.is_empty(), "nothing runs, nothing is deleted");
}

#[test]
fn activating_a_mutating_menu_action_changes_nothing_before_its_dialog_is_accepted() {
    // Copy, Move, Rename, and Delete each only *open* a setup dialog on
    // activation — no `Effect::RunJob` (the only mutation-triggering effect)
    // is emitted until that dialog's own Confirm (file-action-menu "No
    // mutation without an intervening dialog").
    for hotkey in ['C', 'M', 'R', 'D'] {
        let state = opened_menu_state(&["notes.txt"], "notes.txt");
        let (state, effects) = update(state, Command::FileActionMenuHotkey(hotkey));
        assert!(!effects.iter().any(|e| matches!(e, Effect::RunJob(_))), "hotkey {hotkey:?} must not run a job on activation alone");
        assert!(matches!(state.phase, UiPhase::FileOpSetup(_)), "hotkey {hotkey:?} should have opened a setup dialog, got {:?}", state.phase);
    }
}

#[test]
fn cancelling_the_interposed_destination_dialog_after_move_starts_no_job() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let (state, _) = update(state, Command::FileActionMenuHotkey('M'));
    let (state, effects) = update(state, Command::FileOpCancel);
    assert_eq!(state.phase, UiPhase::Panels);
    assert!(effects.is_empty());
}

// ---------------------------------------------------------------------
// file-action-menu: Open with the associated application
// (open-with-associated-app; design D1–D3)
// ---------------------------------------------------------------------

/// "Open launches the associated application": Enter, Enter on a document —
/// Open is highlighted first, so the immediate Confirm activates it. The
/// menu closes, the target goes out as one fire-and-forget launch effect,
/// and the phase stays Panels (no dialog, no TUI suspension of any kind).
#[test]
fn enter_enter_on_a_document_emits_open_with_association_for_the_target() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("report.pdf", 1)];
    let (state, _) = update(state, Command::Enter);
    assert_eq!(state.file_action_menu().unwrap().selected(), FileActionMenuEntry::Open, "setup precondition: Open highlighted");
    let (state, effects) = update(state, Command::FileActionMenuConfirm);
    assert!(state.file_action_menu().is_none(), "activating Open closes the menu");
    assert_eq!(
        effects,
        vec![Effect::OpenWithAssociation { side: PanelSide::Left, path: PathBuf::from("/left/report.pdf"), cwd: PathBuf::from("/left") }]
    );
    assert_eq!(state.phase, UiPhase::Panels, "the panels stay interactive; no dialog opens");
}

/// "O activates Open": the hotkey activates Open exactly as if it had been
/// highlighted and Enter pressed.
#[test]
fn file_action_menu_o_hotkey_activates_open() {
    let state = opened_menu_state(&["report.pdf"], "report.pdf");
    let (state, effects) = update(state, Command::FileActionMenuHotkey('O'));
    assert!(state.file_action_menu().is_none(), "the hotkey closes the menu");
    assert_eq!(
        effects,
        vec![Effect::OpenWithAssociation { side: PanelSide::Left, path: PathBuf::from("/left/report.pdf"), cwd: PathBuf::from("/left") }]
    );
}

/// "Open on a directory shows it in the file browser": the directory menu
/// lists Open first and activating it hands the directory itself to the
/// platform association (Explorer on Windows) — the panel does not
/// navigate (no `StartListing`).
#[test]
fn open_on_a_directory_hands_the_directory_to_the_file_association_without_navigating() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("src")];
    let (state, _) = update(state, Command::OpenActionMenuAt { side: PanelSide::Left, name: OsString::from("src") });
    let (state, effects) = update(state, Command::FileActionMenuConfirm); // Open is highlighted first
    assert_eq!(effects, vec![Effect::OpenWithAssociation { side: PanelSide::Left, path: PathBuf::from("/left/src"), cwd: PathBuf::from("/left") }]);
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "the panel does not navigate");
}

/// "Open ignores the selection": even opened selection-scoped (right-click
/// on a member of the selection), Open emits exactly one launch effect for
/// the menu's target and leaves the selection untouched (design D3).
#[test]
fn open_on_a_selected_target_emits_one_effect_for_that_target_only() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1), file_entry("b.txt", 2), file_entry("c.txt", 3)];
    for n in ["a.txt", "b.txt", "c.txt"] {
        state.left.selected.insert(OsString::from(n));
    }
    let (state, _) = update(state, Command::OpenActionMenuAt { side: PanelSide::Left, name: OsString::from("b.txt") });
    assert!(state.file_action_menu().unwrap().selection_scoped, "setup precondition: opened on a selection member");

    let (state, effects) = update(state, Command::FileActionMenuHotkey('O'));
    assert_eq!(
        effects,
        vec![Effect::OpenWithAssociation { side: PanelSide::Left, path: PathBuf::from("/left/b.txt"), cwd: PathBuf::from("/left") }],
        "one launch effect, for the menu's target only"
    );
    assert_eq!(state.left.selected.len(), 3, "the selection is untouched");
}

/// "No association is reported inline": the effect's failure reply lands on
/// the status line of the panel the Open action was invoked on, verbatim,
/// and opens no dialog.
#[test]
fn open_with_association_failed_lands_on_the_invoking_panels_last_error() {
    let state = test_state(UiPhase::Panels);
    let (state, effects) = update(
        state,
        Command::OpenWithAssociationFailed { side: PanelSide::Left, message: "No application is associated with data.zzz".to_string() },
    );
    assert!(effects.is_empty());
    assert_eq!(state.left.last_error.as_deref(), Some("No application is associated with data.zzz"));
    assert_eq!(state.right.last_error, None);
    assert_eq!(state.phase, UiPhase::Panels, "no dialog opens");
}

/// application-shell "Launching a slow handler leaves the panels
/// interactive": a late failure lands on the panel the menu acted on even
/// though the user has switched panels since — the reply carries the side,
/// so `state.active` at arrival time is irrelevant (offload-blocking-io
/// design D7).
#[test]
fn open_with_association_failed_lands_on_the_recorded_side_not_the_active_panel() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("report.pdf", 1)];
    let (state, _) = update(state, Command::Enter);
    let (state, effects) = update(state, Command::FileActionMenuConfirm); // Open
    assert!(
        matches!(effects.as_slice(), [Effect::OpenWithAssociation { side: PanelSide::Left, .. }]),
        "the effect records the invoking panel: {effects:?}"
    );
    let (state, _) = update(state, Command::ToggleActivePanel);
    assert_eq!(state.active, PanelSide::Right, "precondition: the user has switched panels");
    let (state, _) = update(state, Command::OpenWithAssociationFailed { side: PanelSide::Left, message: "boom".to_string() });
    assert_eq!(state.left.last_error.as_deref(), Some("boom"), "the failure lands on the panel Open was invoked on");
    assert_eq!(state.right.last_error, None);
}

// ---------------------------------------------------------------------
// M3: F9 menus
// ---------------------------------------------------------------------

#[test]
fn f9_opens_the_bar_with_the_left_pulldown_showing() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::MenuOpen);
    let menu = state.menu().expect("F9 opens the bar");
    assert_eq!(menu.active, MenuId::Left);
    assert!(menu.pulldown_open);
}

#[test]
fn esc_closes_the_pulldown_first_then_the_bar() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::MenuOpen);
    let (state, _) = update(state, Command::MenuCollapse);
    let menu = state.menu().expect("the bar stays open after the first Esc");
    assert!(!menu.pulldown_open);
    assert_eq!(menu.active, MenuId::Left, "the active title stays highlighted");

    let (state, _) = update(state, Command::MenuCollapse);
    assert!(state.menu().is_none(), "the second Esc closes the bar");
}

#[test]
fn hotkey_letter_jumps_to_another_menu_with_its_pulldown_open() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('c'));
    let menu = state.menu().expect("bar still open");
    assert_eq!(menu.active, MenuId::Commands);
    assert!(menu.pulldown_open);
}

#[test]
fn horizontal_movement_keeps_a_pulldown_open_and_wraps() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::MenuOpen);
    let (state, _) = update(state, Command::MenuNextMenu);
    assert_eq!(state.menu().unwrap().active, MenuId::Files);
    assert!(state.menu().unwrap().pulldown_open);

    // Left -> Files -> Commands -> Options -> Right -> wraps to Left.
    let state = (0..4).fold(state, |s, _| update(s, Command::MenuNextMenu).0);
    assert_eq!(state.menu().unwrap().active, MenuId::Left);

    let (state, _) = update(state, Command::MenuPrevMenu);
    assert_eq!(state.menu().unwrap().active, MenuId::Right);
    assert!(state.menu().unwrap().pulldown_open);
}

#[test]
fn vertical_selection_skips_separators() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('f'));
    // Files opens on View, its first item; every shipped item is enabled
    // (pulldown-menu-completion), so only separators are ever skipped.
    assert_eq!(state.menu().unwrap().selected_item().map(|i| i.label), Some("View"));
    let (state, _) = update(state, Command::MenuSelectNext);
    assert_eq!(state.menu().unwrap().selected_item().map(|i| i.label), Some("Edit"));
    let (state, _) = update(state, Command::MenuSelectPrev);
    assert_eq!(state.menu().unwrap().selected_item().map(|i| i.label), Some("View"));
}

#[test]
fn enter_dispatches_the_selected_item_and_closes_the_overlay() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('f'));
    // Files opens on View; Copy is two rows down (View, Edit, Copy).
    let (state, _) = update(state, Command::MenuSelectNext);
    let (state, _) = update(state, Command::MenuSelectNext);
    let (state, _) = update(state, Command::MenuActivate); // Copy

    assert!(state.menu().is_none(), "activating an item closes the whole overlay");
    assert!(
        matches!(state.phase, UiPhase::FileOpSetup(FileOpSetup::DestinationInput { kind: JobKind::Copy, .. })),
        "the item's action ran, got {:?}",
        state.phase
    );
}

#[test]
fn left_menu_sorts_the_left_panel_even_when_the_right_one_is_focused() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Right;
    state.left.entries = vec![file_entry("b", 1), file_entry("a", 2)];

    let (state, _) = update(state, Command::MenuOpen); // Left menu
                                                       // Opens on Brief; Down walks Full, Tree, Quick view, Info, then (the
                                                       // separator skipped, not counted) Name, Extension, Modif. time, Size.
    let state = (0..8).fold(state, |s, _| update(s, Command::MenuSelectNext).0);
    assert_eq!(state.menu().unwrap().selected_item().map(|i| i.label), Some("Size"));
    let (state, _) = update(state, Command::MenuActivate);

    assert_eq!(state.left.sort_mode, SortMode::Size, "the Left menu targets the left panel regardless of focus");
    assert_eq!(state.right.sort_mode, SortMode::Name);
}

#[test]
fn right_menu_targets_the_right_panel() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('r'));
    // Opens on Brief; Down four times walks Full, Tree, Quick view, Info.
    let state = (0..4).fold(state, |s, _| update(s, Command::MenuSelectNext).0);
    let (state, effects) = update(state, Command::MenuActivate); // Info
    assert_eq!(state.right.display_mode, DisplayMode::Info);
    assert_eq!(state.left.display_mode, DisplayMode::Full);
    assert!(effects.iter().any(|e| matches!(e, Effect::QueryInfo { panel: PanelSide::Right, .. })));
}

#[test]
fn activating_options_themes_opens_the_theme_picker_and_closes_the_menu() {
    // Options used to be entirely disabled (the old target of this test);
    // Themes (visual-themes) is now its first enabled item, so activating
    // Options and pressing Enter opens the picker instead of doing nothing
    // (theme-selection "Options menu opens the theme picker").
    let (state, _) = update(test_state(UiPhase::Panels), Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('o')); // Options: Themes is first-selectable
    let (state, effects) = update(state, Command::MenuActivate);
    assert!(state.menu().is_none(), "activating an item closes the whole menu overlay");
    assert!(state.theme_picker().is_some(), "Options -> Themes opens the picker");
    assert!(effects.is_empty());
}

#[test]
fn panel_keys_do_not_leak_through_an_open_menu() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a", 1), file_entry("b", 2)];
    let (state, _) = update(state, Command::MenuOpen);
    // MoveCursor is not a menu command; the mapper would never emit it while
    // the bar is open, but the overlay must not be disturbed if it does.
    let (state, _) = update(state, Command::MenuSelectNext);
    assert_eq!(state.left.cursor, 0);
    assert!(state.menu().is_some());
}

#[test]
fn every_menu_item_that_is_enabled_maps_to_a_command() {
    for id in crate::menu::ALL_MENUS {
        for entry in crate::menu::entries(id) {
            if let MenuEntry::Item(item) = entry {
                if item.is_enabled() {
                    assert!(menu_action_command(item.action, PanelSide::Left).is_some(), "`{}` is enabled but dispatches nothing", item.label);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------
// pulldown-menu-completion: every formerly-greyed item wired (task 2.5)
// ---------------------------------------------------------------------

/// pulldown-menus "Every listed item is wired": the exact `Command` each new
/// `MenuAction` stands for. Left/Right-scoped actions carry the menu's own
/// side; Files/Commands actions follow the active panel.
#[test]
fn menu_action_command_maps_the_newly_wired_actions() {
    use crate::menu::MenuAction;
    assert_eq!(menu_action_command(MenuAction::View, PanelSide::Left), Some(Command::RequestViewer));
    assert_eq!(menu_action_command(MenuAction::Edit, PanelSide::Left), Some(Command::RequestEditor));
    assert_eq!(menu_action_command(MenuAction::QuickFilter, PanelSide::Right), Some(Command::QuickFilterStart(PanelSide::Right)));
    assert_eq!(menu_action_command(MenuAction::NewTab, PanelSide::Right), Some(Command::OpenTab(PanelSide::Right)));
    assert_eq!(menu_action_command(MenuAction::CloseTab, PanelSide::Right), Some(Command::CloseTab(PanelSide::Right)));
    assert_eq!(menu_action_command(MenuAction::History, PanelSide::Left), Some(Command::HistoryDialogOpen));
    assert_eq!(menu_action_command(MenuAction::SwapPanels, PanelSide::Left), Some(Command::SwapPanels));
    assert_eq!(menu_action_command(MenuAction::EditMenuFile, PanelSide::Left), Some(Command::EditUserMenuFile));
}

/// pulldown-menus "Files → View opens the viewer": activating Files → View
/// closes the menu and opens the F3 viewer on the cursor entry, exactly as
/// F3 would.
#[test]
fn files_menu_view_follows_the_f3_path() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("notes.txt", 1)];
    let (state, _) = update(state, Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('f'));
    assert_eq!(state.menu().unwrap().selected_item().map(|i| i.label), Some("View"));
    let (state, effects) = update(state, Command::MenuActivate);
    assert!(state.menu().is_none(), "activating an item closes the whole overlay");
    assert_eq!(effects, vec![Effect::OpenViewer { request: 3, path: PathBuf::from("/left/notes.txt") }]);
}

/// Files → Edit follows the F4 path: with no external editor configured, the
/// built-in editor opens on the cursor entry.
#[test]
fn files_menu_edit_follows_the_f4_path() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("notes.txt", 1)];
    let (state, _) = update(state, Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('f'));
    let (state, _) = update(state, Command::MenuSelectNext); // Edit
    let (state, effects) = update(state, Command::MenuActivate);
    assert!(state.menu().is_none());
    assert_eq!(effects, vec![Effect::OpenEditor { request: 3, path: PathBuf::from("/left/notes.txt") }]);
}

/// pulldown-menus "Right → Filter on the inactive panel": the filter starts
/// on the right panel, which becomes active so typed characters narrow it
/// (design D1).
#[test]
fn right_menu_filter_activates_the_right_panel_with_its_quick_filter_open() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    let (state, _) = update(state, Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('r'));
    let filter_index = crate::menu::entries(MenuId::Right).iter().position(|e| matches!(e, MenuEntry::Item(i) if i.label == "Filter")).unwrap();
    let mut state = state;
    state.menu_mut().unwrap().selected = filter_index;
    let (state, _) = update(state, Command::MenuActivate);
    assert!(state.menu().is_none());
    assert_eq!(state.right.quick_filter.as_deref(), Some(""), "the right panel's quick filter opened");
    assert_eq!(state.left.quick_filter, None, "the left panel is untouched");
    assert_eq!(state.active, PanelSide::Right, "the filtered panel became active so typing reaches it");
    // Subsequent typed characters narrow the right panel.
    let (state, _) = update(state, Command::QuickFilterChar('x'));
    assert_eq!(state.right.quick_filter.as_deref(), Some("x"));
}

/// pulldown-menus "Left → New tab on the inactive panel": the left panel
/// gains the tab exactly as Ctrl+T with the left panel active would have
/// created it, and focus stays put (design D1).
#[test]
fn left_menu_new_tab_opens_a_tab_on_the_left_without_moving_focus() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Right;
    let (state, _) = update(state, Command::MenuOpen); // the Left menu is first
    let tab_index = crate::menu::entries(MenuId::Left).iter().position(|e| matches!(e, MenuEntry::Item(i) if i.label == "New tab")).unwrap();
    let mut state = state;
    state.menu_mut().unwrap().selected = tab_index;
    let (state, _) = update(state, Command::MenuActivate);
    assert!(state.menu().is_none());
    assert_eq!(state.left.tab_count(), 2, "the left panel gained a tab");
    assert_eq!(state.right.tab_count(), 1, "the right panel is untouched");
    assert_eq!(state.active, PanelSide::Right, "focus stays on the right panel");
}

/// Left → Close tab closes a tab on the left panel regardless of focus.
#[test]
fn left_menu_close_tab_closes_a_left_tab_without_moving_focus() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Right;
    state.left.open_tab();
    let (state, _) = update(state, Command::MenuOpen);
    let close_index = crate::menu::entries(MenuId::Left).iter().position(|e| matches!(e, MenuEntry::Item(i) if i.label == "Close tab")).unwrap();
    let mut state = state;
    state.menu_mut().unwrap().selected = close_index;
    let (state, _) = update(state, Command::MenuActivate);
    assert_eq!(state.left.tab_count(), 1, "the left panel's tab closed");
    assert_eq!(state.active, PanelSide::Right, "focus stays on the right panel");
}

/// Commands → History opens the Alt+F8 command-history dialog.
#[test]
fn commands_menu_history_opens_the_history_dialog() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('c'));
    let history_index = crate::menu::entries(MenuId::Commands).iter().position(|e| matches!(e, MenuEntry::Item(i) if i.label == "History")).unwrap();
    let mut state = state;
    state.menu_mut().unwrap().selected = history_index;
    let (state, _) = update(state, Command::MenuActivate);
    assert!(state.menu().is_none());
    assert!(state.history_dialog().is_some(), "Commands -> History opens the dialog");
}

/// Commands → Swap panels issues the swap (see the swap tests below).
#[test]
fn commands_menu_swap_panels_swaps_the_panels() {
    let mut state = test_state(UiPhase::Panels);
    complete_panel_listings(&mut state);
    let (state, _) = update(state, Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('c'));
    let swap_index = crate::menu::entries(MenuId::Commands).iter().position(|e| matches!(e, MenuEntry::Item(i) if i.label == "Swap panels")).unwrap();
    let mut state = state;
    state.menu_mut().unwrap().selected = swap_index;
    let (state, _) = update(state, Command::MenuActivate);
    assert!(state.menu().is_none());
    assert_eq!(state.left.cwd, PathBuf::from("/right"));
    assert_eq!(state.right.cwd, PathBuf::from("/left"));
}

/// Commands → Menu file edit opens the user-menu file in the editor (see the
/// menu-file edit tests below).
#[test]
fn commands_menu_menu_file_edit_opens_the_user_menu_file_in_the_editor() {
    let mut state = test_state(UiPhase::Panels);
    state.user_menu_path = PathBuf::from("/profile/usermenu.toml");
    let (state, _) = update(state, Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('c'));
    let edit_index =
        crate::menu::entries(MenuId::Commands).iter().position(|e| matches!(e, MenuEntry::Item(i) if i.label == "Menu file edit")).unwrap();
    let mut state = state;
    state.menu_mut().unwrap().selected = edit_index;
    let (state, effects) = update(state, Command::MenuActivate);
    assert!(state.menu().is_none());
    assert_eq!(effects, vec![Effect::OpenEditor { request: 3, path: PathBuf::from("/profile/usermenu.toml") }]);
}

// ---------------------------------------------------------------------
// pulldown-menu-completion: the Alt+F8 command-history dialog (task 2.5;
// command-line "Command history dialog (Alt+F8)"; design D2)
// ---------------------------------------------------------------------

fn history_state(entries: &[&str]) -> State {
    let mut state = test_state(UiPhase::Panels);
    state.history = entries.iter().map(|s| s.to_string()).collect();
    state
}

#[test]
fn history_dialog_open_lists_history_newest_first() {
    let state = history_state(&["cd C:\\work", "cd D:\\"]);
    let (state, effects) = update(state, Command::HistoryDialogOpen);
    assert!(effects.is_empty());
    let dialog = state.history_dialog().expect("the dialog opens");
    assert_eq!(dialog.cursor, 0, "the highlight starts on the newest entry");
}

#[test]
fn history_dialog_confirm_places_the_entry_with_the_caret_at_the_end() {
    // Spec scenario "Pick an entry": history `cd C:\work` then `cd D:\`;
    // Alt+F8, Down, Enter -> the buffer reads `cd C:\work`, caret at end.
    let state = history_state(&["cd C:\\work", "cd D:\\"]);
    let (state, _) = update(state, Command::HistoryDialogOpen);
    let (state, _) = update(state, Command::HistoryDialogMove(1));
    let (state, effects) = update(state, Command::HistoryDialogConfirm);
    assert!(state.history_dialog().is_none(), "Enter closes the dialog");
    assert!(effects.is_empty(), "nothing runs — the entry is only placed");
    assert_eq!(state.command_line.text(), "cd C:\\work");
    assert_eq!(state.command_line.caret(), "cd C:\\work".chars().count(), "the caret sits at the end");
    assert_eq!(state.history_cursor, None, "no history recall is in progress");
}

#[test]
fn history_dialog_confirm_ends_an_up_down_recall_in_progress() {
    let mut state = history_state(&["cd C:\\work", "cd D:\\"]);
    state.command_line = LineEdit::from("cd D:\\");
    state.history_cursor = Some(1);
    let (state, _) = update(state, Command::HistoryDialogOpen);
    let (state, _) = update(state, Command::HistoryDialogConfirm); // cursor 0 = newest = "cd D:\"
    assert_eq!(state.command_line.text(), "cd D:\\");
    assert_eq!(state.history_cursor, None);
}

#[test]
fn history_dialog_cancel_leaves_the_buffer_alone() {
    // Spec scenario "Esc leaves the buffer alone".
    let mut state = history_state(&["cd C:\\work"]);
    state.command_line = LineEdit::from("cd x");
    let (state, _) = update(state, Command::HistoryDialogOpen);
    let (state, effects) = update(state, Command::HistoryDialogCancel);
    assert!(state.history_dialog().is_none(), "Esc closes the dialog");
    assert!(effects.is_empty());
    assert_eq!(state.command_line.text(), "cd x", "the buffer is unchanged");
}

#[test]
fn history_dialog_empty_history_shows_no_history_and_enter_does_nothing() {
    // Spec scenario "Empty history": Enter leaves the buffer unchanged with
    // the dialog open until Esc.
    let state = history_state(&[]);
    let (state, _) = update(state, Command::HistoryDialogOpen);
    let before = state.command_line.clone();
    let (state, effects) = update(state, Command::HistoryDialogConfirm);
    assert!(state.history_dialog().is_some(), "Enter on the empty history keeps the dialog open");
    assert!(effects.is_empty());
    assert_eq!(state.command_line, before, "the buffer is unchanged");
    let (state, _) = update(state, Command::HistoryDialogCancel);
    assert!(state.history_dialog().is_none(), "Esc still closes it");
}

#[test]
fn history_dialog_move_clamps_at_both_ends() {
    let state = history_state(&["one", "two", "three"]);
    let (state, _) = update(state, Command::HistoryDialogOpen);
    let (state, _) = update(state, Command::HistoryDialogMove(-1));
    assert_eq!(state.history_dialog().unwrap().cursor, 0, "Up at the top stays put");
    let (state, _) = update(state, Command::HistoryDialogMove(isize::MAX / 2)); // End/PgDn
    assert_eq!(state.history_dialog().unwrap().cursor, 2, "End lands on the oldest entry");
    let (state, _) = update(state, Command::HistoryDialogMove(1));
    assert_eq!(state.history_dialog().unwrap().cursor, 2, "Down at the bottom stays put");
}

#[test]
fn history_dialog_commands_without_an_open_dialog_are_no_ops() {
    // The input mapper only emits these while the dialog is open; the
    // reducer still never acts on them otherwise (same convention as the
    // other overlay command families).
    let state = history_state(&["cd C:\\work"]);
    for cmd in [Command::HistoryDialogMove(1), Command::HistoryDialogConfirm, Command::HistoryDialogCancel] {
        let before = state.clone();
        let (after, effects) = update(before.clone(), cmd);
        assert!(effects.is_empty());
        assert_eq!(after, before);
    }
}

// ---------------------------------------------------------------------
// M3: drive select
// ---------------------------------------------------------------------

#[test]
fn alt_f1_and_alt_f2_target_their_own_panels() {
    for side in [PanelSide::Left, PanelSide::Right] {
        let (_, effects) = update(test_state(UiPhase::Panels), Command::OpenDriveSelect(side));
        assert_eq!(effects, vec![Effect::EnumerateDrives(side)]);
    }
}

#[test]
fn drive_list_opens_the_dialog_and_requests_every_label_lazily() {
    let (state, effects) = update(test_state(UiPhase::Panels), Command::DriveListReady { target: PanelSide::Left, drives: vec!['A', 'C', 'D'] });
    let dialog = state.drive_select().expect("the dialog opened");
    assert_eq!(dialog.target, PanelSide::Left);
    assert_eq!(dialog.drives.len(), 3, "every letter is present on the first frame");
    assert!(dialog.drives.iter().all(|d| d.label.is_none()), "labels are still pending");
    let generation = dialog.generation;
    assert_eq!(
        effects,
        vec![
            Effect::FetchDriveLabel { target: PanelSide::Left, letter: 'A', generation },
            Effect::FetchDriveLabel { target: PanelSide::Left, letter: 'C', generation },
            Effect::FetchDriveLabel { target: PanelSide::Left, letter: 'D', generation },
        ]
    );
}

#[test]
fn the_dialog_opens_on_the_target_panel_s_current_drive() {
    let mut state = test_state(UiPhase::Panels);
    state.right.cwd = PathBuf::from(r"D:\work");
    let (state, _) = update(state, Command::DriveListReady { target: PanelSide::Right, drives: vec!['C', 'D'] });
    assert_eq!(state.drive_select().unwrap().selected_letter(), Some('D'));
}

#[test]
fn a_resolved_label_fills_in_place_without_moving_other_rows() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::DriveListReady { target: PanelSide::Left, drives: vec!['A', 'C'] });
    let generation = state.drive_select().unwrap().generation;
    let (state, effects) =
        update(state, Command::DriveLabelResolved { target: PanelSide::Left, letter: 'C', label: Some("OS".to_string()), generation });
    assert!(effects.is_empty());
    let dialog = state.drive_select().unwrap();
    assert_eq!(dialog.drives, vec![DriveEntry { letter: 'A', label: None }, DriveEntry { letter: 'C', label: Some("OS".to_string()) }]);
}

#[test]
fn a_label_arriving_after_the_dialog_closed_is_discarded() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::DriveListReady { target: PanelSide::Left, drives: vec!['C'] });
    let generation = state.drive_select().unwrap().generation;
    let (state, _) = update(state, Command::DriveSelectCancel);
    assert!(state.drive_select().is_none());

    let (state, effects) =
        update(state, Command::DriveLabelResolved { target: PanelSide::Left, letter: 'C', label: Some("OS".to_string()), generation });
    assert!(state.drive_select().is_none(), "a stale result must not resurrect the dialog");
    assert!(effects.is_empty());
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "and must not touch the panel");
}

#[test]
fn a_label_for_a_superseded_target_panel_is_discarded() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::DriveListReady { target: PanelSide::Left, drives: vec!['C'] });
    let first_generation = state.drive_select().unwrap().generation;
    // The dialog was reopened for the other panel before the label landed.
    let (state, _) = update(state, Command::DriveListReady { target: PanelSide::Right, drives: vec!['C'] });
    let (state, _) = update(
        state,
        Command::DriveLabelResolved { target: PanelSide::Left, letter: 'C', label: Some("OS".to_string()), generation: first_generation },
    );
    assert_eq!(state.drive_select().unwrap().drives[0].label, None);
}

#[test]
fn a_stale_drive_label_from_a_quick_reopen_of_the_same_target_is_discarded_but_the_fresher_one_applies() {
    // Regression coverage for the out-of-order-completion race: the user
    // cancels and reopens the drive dialog for the *same* panel before the
    // first fetch lands (Alt+F1, Esc, Alt+F1 again). `target` alone can't
    // tell the two sessions apart — only `generation` can.
    let (state, _) = update(test_state(UiPhase::Panels), Command::DriveListReady { target: PanelSide::Left, drives: vec!['C'] });
    let first_generation = state.drive_select().unwrap().generation;

    let (state, _) = update(state, Command::DriveSelectCancel);
    let (state, _) = update(state, Command::DriveListReady { target: PanelSide::Left, drives: vec!['C'] });
    let second_generation = state.drive_select().unwrap().generation;
    assert_ne!(first_generation, second_generation, "reopening mints a fresh generation");

    let (state, _) = update(
        state,
        Command::DriveLabelResolved { target: PanelSide::Left, letter: 'C', label: Some("STALE".to_string()), generation: first_generation },
    );
    assert_eq!(state.drive_select().unwrap().drives[0].label, None, "the stale (first-generation) answer is dropped");

    let (state, _) = update(
        state,
        Command::DriveLabelResolved { target: PanelSide::Left, letter: 'C', label: Some("FRESH".to_string()), generation: second_generation },
    );
    assert_eq!(state.drive_select().unwrap().drives[0].label, Some("FRESH".to_string()), "the current generation's answer applies");
}

#[test]
fn esc_dismisses_the_dialog_leaving_the_panel_where_it_was() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\Users");
    let (state, _) = update(state, Command::DriveListReady { target: PanelSide::Left, drives: vec!['C', 'D'] });
    let (state, effects) = update(state, Command::DriveSelectCancel);
    assert!(state.drive_select().is_none());
    assert!(effects.is_empty());
    assert_eq!(state.left.cwd, PathBuf::from(r"C:\Users"));
}

#[test]
fn selecting_a_drive_switches_the_target_panel_to_its_root() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::DriveListReady { target: PanelSide::Right, drives: vec!['C', 'D'] });
    let (state, _) = update(state, Command::DriveSelectMove(1));
    let (state, effects) = update(state, Command::DriveSelectConfirm);
    assert!(state.drive_select().is_none(), "confirming closes the dialog");
    assert_eq!(state.right.cwd, PathBuf::from(r"D:\"));
    assert_eq!(without_git_info_effects(effects), vec![start_listing(&state, PanelSide::Right, r"D:\")]);
}

#[test]
fn selecting_an_unavailable_drive_surfaces_the_panel_error_state() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::DriveListReady { target: PanelSide::Left, drives: vec!['A'] });
    let (state, effects) = update(state, Command::DriveSelectConfirm);
    assert_eq!(without_git_info_effects(effects), vec![start_listing(&state, PanelSide::Left, r"A:\")]);
    // The worker's read fails and comes back as a normal listing failure —
    // no dedicated hang-prone path.
    let (state, _) = listing_failed(state, PanelSide::Left, "device not ready".into());
    assert_eq!(state.left.last_error.as_deref(), Some("device not ready"));
}

#[test]
fn drive_select_claims_navigation_keys_while_open() {
    let (state, _) = update(test_state(UiPhase::Panels), Command::DriveListReady { target: PanelSide::Left, drives: vec!['A', 'C', 'D'] });
    let (state, _) = update(state, Command::DriveSelectMove(2));
    assert_eq!(state.drive_select().unwrap().selected_letter(), Some('D'));
    let (state, _) = update(state, Command::DriveSelectMove(-1));
    assert_eq!(state.drive_select().unwrap().selected_letter(), Some('C'));
}

#[test]
fn fuzzy_jump_open_and_esc_close_without_navigating() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::FuzzyJumpOpen);
    assert!(state.fuzzy_jump().is_some());
    let (state, effects) = update(state, Command::FuzzyJumpCancel);
    assert!(state.fuzzy_jump().is_none());
    assert!(effects.is_empty());
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "the panel is unchanged");
}

#[test]
fn fuzzy_jump_typing_and_backspace_edit_the_pattern() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::FuzzyJumpOpen);
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Insert('d')));
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Insert('o')));
    assert_eq!(state.fuzzy_jump().unwrap().pattern.text(), "do");
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Backspace));
    assert_eq!(state.fuzzy_jump().unwrap().pattern.text(), "d");
}

#[test]
fn fuzzy_jump_confirm_navigates_the_active_panel_to_the_highlighted_directory() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Right;
    state.dir_history = vec![history_entry(r"C:\Low", 1, 0), history_entry(r"C:\High", 10, 0)];
    let (state, _) = update(state, Command::FuzzyJumpOpen);
    // No pattern typed: the full frecency-ranked list shows, most frecent
    // first, so the default (cursor 0) highlight is `C:\High`.
    let (state, effects) = update(state, Command::FuzzyJumpConfirm);
    assert!(state.fuzzy_jump().is_none(), "the dialog closes");
    assert_eq!(state.right.cwd, PathBuf::from(r"C:\High"));
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "the opposite panel is unaffected");
    assert!(without_git_info_effects(effects).iter().any(|e| matches!(e, Effect::StartListing { panel: PanelSide::Right, .. })));
}

#[test]
fn fuzzy_jump_move_clamps_within_the_currently_filtered_list() {
    let mut state = test_state(UiPhase::Panels);
    state.dir_history = vec![history_entry(r"C:\A", 1, 0), history_entry(r"C:\B", 1, 0)];
    let (state, _) = update(state, Command::FuzzyJumpOpen);
    let (state, _) = update(state, Command::FuzzyJumpMove(-5));
    assert_eq!(state.fuzzy_jump().unwrap().cursor, 0);
    let (state, _) = update(state, Command::FuzzyJumpMove(5));
    assert_eq!(state.fuzzy_jump().unwrap().cursor, 1);
}

/// R-13 regression (fuzzy-jump "Recency survives a restart"; history-
/// persistence-hardening design D1): an earlier session stamped `C:\old`
/// at epoch E and `C:\new` three days later. A fresh `State` — monotonic
/// clock back near zero, wall clock a minute after the last visit — ranks
/// `C:\new` first, because ranking reads `epoch_ms`, not `clock_ms`.
#[test]
fn fuzzy_jump_ranks_with_the_epoch_clock_so_recency_survives_a_restart() {
    let e = 1_787_000_000_000u64;
    let day = 24 * 60 * 60 * 1000;
    let mut state = test_state(UiPhase::Panels);
    state.clock_ms = 500; // a fresh process: the monotonic clock restarted
    state.epoch_ms = e + 3 * day + 60_000;
    state.dir_history = vec![history_entry(r"C:\old", 3, e), history_entry(r"C:\new", 3, e + 3 * day)];
    let (state, _) = update(state, Command::FuzzyJumpOpen);
    let (state, effects) = update(state, Command::FuzzyJumpConfirm);
    assert_eq!(state.left.cwd, PathBuf::from(r"C:\new"), "Enter on the top row lands on the more recently visited directory");
    assert!(effects.iter().any(|e| matches!(e, Effect::StartListing { path, .. } if path == Path::new(r"C:\new"))));
}

/// A tick carries both clocks, and a navigation stamps its visit with the
/// epoch reading (design D1).
#[test]
fn tick_updates_both_clocks_and_visits_are_stamped_with_epoch_time() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    let (state, _) = update(state, Command::Tick { now_ms: 42, epoch_ms: 1_787_000_000_000 });
    assert_eq!(state.clock_ms, 42);
    assert_eq!(state.epoch_ms, 1_787_000_000_000);
    let (state, _) = update(state, Command::Enter);
    assert_eq!(
        state.dir_history,
        vec![history_entry("/left/sub", 1, 1_787_000_000_000)],
        "the visit carries wall-clock time, not the monotonic reading"
    );
}

// ---------------------------------------------------------------------
// M5: Alt+F7 find file
// ---------------------------------------------------------------------

fn find_match(rel: &str, name: &str) -> FindMatch {
    FindMatch { relative_path: PathBuf::from(rel), entry: dir_entry_named(name) }
}

fn dir_entry_named(name: &str) -> Entry {
    Entry::new(OsString::from(name), EntryKind::File, 0, None, false, false)
}

#[test]
fn find_file_open_roots_at_the_active_panels_directory() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Right;
    state.right.cwd = PathBuf::from(r"C:\proj");
    let (state, _) = update(state, Command::FindFileOpen);
    assert_eq!(state.find_file().unwrap().root, PathBuf::from(r"C:\proj"));
}

#[test]
fn find_file_submit_mints_a_request_and_dispatches_the_walk_effect() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\proj");
    let (state, _) = update(state, Command::FindFileOpen);
    let (state, _) = update(state, Command::TextEdit(TextEditOp::Insert('r')));
    let (state, effects) = update(state, Command::FindFileSubmit);
    match effects.as_slice() {
        [Effect::FindInSubtree { root, pattern, request }] => {
            assert_eq!(root, &PathBuf::from(r"C:\proj"));
            assert_eq!(pattern, "r");
            assert_eq!(state.find_file().unwrap().request, Some(*request));
        }
        other => panic!("expected exactly one FindInSubtree effect, got {other:?}"),
    }
}

#[test]
fn find_file_matches_from_a_superseded_request_are_dropped() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::FindFileOpen);
    let (state, _) = update(state, Command::FindFileSubmit); // request 1
    let (state, _) = update(state, Command::FindFileMatch { request: 999, m: find_match("a.txt", "a.txt") });
    assert!(state.find_file().unwrap().results.is_empty(), "a stale-request match must be dropped");
}

#[test]
fn find_file_confirm_navigates_in_place_and_seeds_the_cursor_target() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\proj");
    let (state, _) = update(state, Command::FindFileOpen);
    let (state, _) = update(state, Command::FindFileSubmit);
    let request = state.find_file().unwrap().request.unwrap();
    let (state, _) = update(state, Command::FindFileMatch { request, m: find_match(r"sub\report.txt", "report.txt") });
    let (state, effects) = update(state, Command::FindFileConfirm);
    assert!(state.find_file().is_none(), "the dialog closes");
    assert_eq!(state.left.cwd, PathBuf::from(r"C:\proj\sub"), "navigates to the match's containing directory");
    assert_eq!(state.left.pending_cursor_target, Some(OsString::from("report.txt")));
    assert!(without_git_info_effects(effects).iter().any(|e| matches!(e, Effect::StartListing { panel: PanelSide::Left, .. })));
}

#[test]
fn find_file_confirm_with_a_root_level_match_navigates_to_the_root() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from(r"C:\proj");
    let (state, _) = update(state, Command::FindFileOpen);
    let (state, _) = update(state, Command::FindFileSubmit);
    let request = state.find_file().unwrap().request.unwrap();
    let (state, _) = update(state, Command::FindFileMatch { request, m: find_match("readme.txt", "readme.txt") });
    let (state, _) = update(state, Command::FindFileConfirm);
    assert_eq!(state.left.cwd, PathBuf::from(r"C:\proj"));
}

#[test]
fn find_file_cancel_abandons_the_search_without_navigating() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::FindFileOpen);
    let (state, _) = update(state, Command::FindFileSubmit);
    let request = state.find_file().unwrap().request.unwrap();
    let (state, effects) = update(state, Command::FindFileCancel);
    assert!(state.find_file().is_none());
    // The running walk is told to stop (find-file "Esc stops the walk");
    // nothing else — in particular no navigation — happens.
    assert_eq!(effects, vec![Effect::CancelRequest(request)]);
    assert_eq!(state.left.cwd, PathBuf::from("/left"));
}

// ---------------------------------------------------------------------
// listing-request-guard: find-file and viewer searches stop when dismissed
// or superseded (find-file "Dismissed or superseded searches stop"; viewer
// "Search stops when superseded or when the viewer closes"; tasks 5.8/5.9)
// ---------------------------------------------------------------------

#[test]
fn find_file_cancel_and_confirm_stop_a_running_walk_but_not_a_finished_one() {
    // Esc while the walk runs: `CancelRequest(request)`.
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::FindFileOpen);
    let (state, _) = update(state, Command::FindFileSubmit);
    let request = state.find_file().unwrap().request.unwrap();
    let (_, effects) = update(state.clone(), Command::FindFileCancel);
    assert_eq!(effects, vec![Effect::CancelRequest(request)], "Esc stops the walk");

    // Enter on a result while the walk runs: the cancel precedes the
    // navigation's own effects (find-file "Confirming a result while the
    // walk is running stops it").
    let (state, _) = update(state, Command::FindFileMatch { request, m: find_match(r"sub\report.txt", "report.txt") });
    let (confirmed, effects) = update(state.clone(), Command::FindFileConfirm);
    assert_eq!(effects.first(), Some(&Effect::CancelRequest(request)), "the running walk is stopped first, got {effects:?}");
    assert!(effects.iter().any(|e| matches!(e, Effect::StartListing { panel: PanelSide::Left, .. })), "the navigation still happens");
    assert_eq!(confirmed.left.cwd, PathBuf::from("/left/sub"));

    // Once the walk has reported done there is nothing to stop: neither
    // Esc nor Enter cancels it. (Enter's navigation still cancels the
    // panel's own still-streaming *listing* — a different request.)
    let (state, _) = update(state, Command::FindFileSearchDone { request });
    let (_, effects) = update(state.clone(), Command::FindFileCancel);
    assert!(effects.is_empty(), "a finished walk is not cancelled: {effects:?}");
    let (_, effects) = update(state, Command::FindFileConfirm);
    assert!(!effects.contains(&Effect::CancelRequest(request)), "a finished walk is not cancelled: {effects:?}");
}

#[test]
fn find_file_cancel_before_any_search_was_submitted_emits_nothing() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::FindFileOpen);
    let (state, effects) = update(state, Command::FindFileCancel);
    assert!(state.find_file().is_none());
    assert!(effects.is_empty(), "no walk was ever started, so there is nothing to stop");
}

#[test]
fn closing_the_viewer_or_starting_a_newer_search_stops_the_earlier_one() {
    // viewer "A newer search supersedes a running one": the second F7
    // confirm cancels the first id before minting its own.
    let state = opened_viewer_state(1000);
    let (state, first) = viewer_with_pattern_confirmed(state, "ab");
    let (state, _) = update(state, Command::ViewerSearchStart);
    let (state, _) = update(state, Command::ViewerSearchChar('c'));
    let (state, effects) = update(state, Command::ViewerSearchConfirm);
    let UiPhase::Viewer(v) = &state.phase else { panic!("expected viewer phase") };
    let second = v.search_request.expect("the second search mints its own id");
    assert_ne!(first, second);
    assert_eq!(
        effects,
        vec![
            Effect::CancelRequest(first),
            Effect::RunViewerSearch { path: PathBuf::from("/left/f.txt"), start_offset: 0, pattern: b"c".to_vec(), request: second },
        ]
    );

    // viewer "Closing the viewer stops a running search": F10 cancels the
    // outstanding id. The id stays recorded after a result arrives, so the
    // cancel is emitted then too — the TUI's registry treats an id with no
    // running worker as a no-op (design D3).
    let (state, effects) = update(state, Command::ViewerClose);
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(effects, vec![Effect::CancelRequest(second)]);

    // A viewer that never searched has nothing to cancel.
    let (_, effects) = update(opened_viewer_state(1000), Command::ViewerClose);
    assert!(effects.is_empty());
}

#[test]
fn listing_complete_settles_the_cursor_on_a_pending_find_file_target() {
    let mut state = test_state(UiPhase::Panels);
    state.left.pending_cursor_target = Some(OsString::from("b.txt"));
    let (state, _) = listing_chunk(state, PanelSide::Left, vec![dir_entry_named("a.txt"), dir_entry_named("b.txt"), dir_entry_named("c.txt")]);
    let (state, _) = listing_complete(state, PanelSide::Left, 3);
    assert_eq!(state.left.cursor, 1);
    assert!(state.left.pending_cursor_target.is_none(), "consumed once applied");
}

// ---------------------------------------------------------------------
// M5: F2 user menu
// ---------------------------------------------------------------------

#[test]
fn user_menu_confirm_dispatches_the_highlighted_commands_shell_passthrough() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Right;
    state.right.cwd = PathBuf::from(r"C:\Projects\app");
    state.user_menu_entries = vec![
        crate::config::UserMenuEntry { label: "A".to_string(), command: "echo a".to_string() },
        crate::config::UserMenuEntry { label: "Build".to_string(), command: "cargo build".to_string() },
    ];
    let (state, _) = update(state, Command::UserMenuOpen);
    let (state, _) = update(state, Command::UserMenuMove(1));
    let (state, effects) = update(state, Command::UserMenuConfirm);
    assert!(state.user_menu().is_none(), "the menu closes");
    match effects.as_slice() {
        [Effect::RunShellCommand(inv, side)] => {
            assert_eq!(inv.cwd, PathBuf::from(r"C:\Projects\app"));
            assert!(inv.args.iter().any(|a| a.contains("cargo build")), "{:?}", inv.args);
            assert_eq!(*side, PanelSide::Right);
        }
        other => panic!("expected exactly one RunShellCommand effect, got {other:?}"),
    }
}

#[test]
fn user_menu_esc_closes_without_running_anything() {
    let mut state = test_state(UiPhase::Panels);
    state.user_menu_entries = vec![crate::config::UserMenuEntry { label: "A".to_string(), command: "echo a".to_string() }];
    let (state, _) = update(state, Command::UserMenuOpen);
    let (state, effects) = update(state, Command::UserMenuCancel);
    assert!(state.user_menu().is_none());
    assert!(effects.is_empty());
}

#[test]
fn user_menu_move_clamps_and_confirm_on_an_empty_menu_is_a_harmless_close() {
    let state = test_state(UiPhase::Panels); // user_menu_entries is empty
    let (state, _) = update(state, Command::UserMenuOpen);
    let (state, _) = update(state, Command::UserMenuMove(5));
    assert_eq!(state.user_menu().unwrap().cursor, 0);
    let (state, effects) = update(state, Command::UserMenuConfirm);
    assert!(state.user_menu().is_none());
    assert!(effects.is_empty(), "nothing to run on an empty menu");
}

// ---------------------------------------------------------------------
// user-menu-themes-entry: built-in Themes slot
// ---------------------------------------------------------------------

#[test]
fn user_menu_down_past_the_last_user_entry_lands_on_themes_and_clamps_there() {
    let mut state = test_state(UiPhase::Panels);
    state.user_menu_entries = vec![
        crate::config::UserMenuEntry { label: "A".to_string(), command: "echo a".to_string() },
        crate::config::UserMenuEntry { label: "B".to_string(), command: "echo b".to_string() },
    ];
    let (state, _) = update(state, Command::UserMenuOpen);
    // Two user entries (indices 0, 1) plus the built-in slot at index 2.
    let (state, _) = update(state, Command::UserMenuMove(2));
    assert_eq!(state.user_menu().unwrap().cursor, 2, "Down from the last user entry lands on the built-in Themes slot");
    let (state, _) = update(state, Command::UserMenuMove(1));
    assert_eq!(state.user_menu().unwrap().cursor, 2, "the built-in slot is the end of the domain: Down holds, it does not wrap");
}

#[test]
fn user_menu_confirm_on_themes_opens_the_picker_pre_highlighted_with_no_shell_effect() {
    let mut state = test_state(UiPhase::Panels);
    state.theme = Theme::terminal_green();
    state.user_menu_entries = vec![crate::config::UserMenuEntry { label: "A".to_string(), command: "echo a".to_string() }];
    let (state, _) = update(state, Command::UserMenuOpen);
    let (state, _) = update(state, Command::UserMenuMove(1)); // off the one user entry, onto the built-in slot
    assert_eq!(state.user_menu().unwrap().cursor, 1);
    let (state, effects) = update(state, Command::UserMenuConfirm);
    assert!(state.user_menu().is_none(), "the F2 menu closes");
    let picker = state.theme_picker().expect("Confirm on the built-in slot opens the theme picker");
    let expected = crate::theme::BUILTIN_THEME_NAMES.iter().position(|n| *n == "terminal-green").unwrap();
    assert_eq!(picker.highlight, expected, "the active theme's row is pre-highlighted, same as the Options -> Themes route");
    assert!(effects.is_empty(), "no shell effect is emitted for the built-in slot");
}

#[test]
fn user_menu_confirm_on_a_user_entry_still_runs_it_via_the_shell_unaffected_by_the_built_in_slot() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Right;
    state.right.cwd = PathBuf::from(r"C:\Projects\app");
    state.user_menu_entries = vec![crate::config::UserMenuEntry { label: "Build".to_string(), command: "cargo build".to_string() }];
    let (state, _) = update(state, Command::UserMenuOpen);
    let (state, effects) = update(state, Command::UserMenuConfirm);
    assert!(state.user_menu().is_none());
    assert!(state.theme_picker().is_none(), "a user entry never opens the theme picker");
    match effects.as_slice() {
        [Effect::RunShellCommand(inv, side)] => {
            assert_eq!(inv.cwd, PathBuf::from(r"C:\Projects\app"));
            assert!(inv.args.iter().any(|a| a.contains("cargo build")), "{:?}", inv.args);
            assert_eq!(*side, PanelSide::Right);
        }
        other => panic!("expected exactly one RunShellCommand effect, got {other:?}"),
    }
}

#[test]
fn theme_picker_esc_after_f2_origin_returns_to_the_panels_without_reopening_the_user_menu() {
    let mut state = test_state(UiPhase::Panels);
    state.user_menu_entries = vec![crate::config::UserMenuEntry { label: "A".to_string(), command: "echo a".to_string() }];
    let (state, _) = update(state, Command::UserMenuOpen);
    let (state, _) = update(state, Command::UserMenuMove(1)); // onto the built-in Themes slot
    let (state, _) = update(state, Command::UserMenuConfirm);
    assert!(state.user_menu().is_none() && state.theme_picker().is_some());
    let (state, effects) = update(state, Command::ThemePickerCancel);
    assert!(state.theme_picker().is_none(), "the picker closes");
    assert!(state.user_menu().is_none(), "Esc lands on the panels, not back on the F2 menu (design D5)");
    assert!(effects.is_empty());
}

// ---------------------------------------------------------------------
// visual-themes: Options -> Themes picker
// ---------------------------------------------------------------------

#[test]
fn theme_picker_opens_from_the_options_menu_with_the_active_theme_highlighted() {
    let mut state = test_state(UiPhase::Panels);
    state.theme = Theme::terminal_green();
    let (state, _) = update(state, Command::MenuOpen);
    let (state, _) = update(state, Command::MenuHotkey('o'));
    let (state, effects) = update(state, Command::MenuActivate);
    assert!(state.menu().is_none(), "the menu overlay closes");
    let picker = state.theme_picker().expect("Options -> Themes opens the picker");
    let expected = crate::theme::BUILTIN_THEME_NAMES.iter().position(|n| *n == "terminal-green").unwrap();
    assert_eq!(picker.highlight, expected, "the active theme's row is pre-highlighted");
    assert!(effects.is_empty());
}

#[test]
fn theme_picker_up_down_moves_the_highlight_over_the_theme_list() {
    let state = test_state(UiPhase::Panels); // active theme is nc-classic (index 0)
    let (state, _) = update(state, Command::ThemePickerOpen);
    assert_eq!(state.theme_picker().unwrap().highlight, 0);
    let (state, _) = update(state, Command::ThemePickerMove(1));
    assert_eq!(state.theme_picker().unwrap().highlight, 1);
    let (state, _) = update(state, Command::ThemePickerMove(-1));
    assert_eq!(state.theme_picker().unwrap().highlight, 0, "Up from the first row holds, it does not wrap");
}

#[test]
fn theme_picker_enter_applies_the_highlighted_theme_immediately_and_persists_it() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::ThemePickerOpen);
    // Move down to `yellow-storm` (index 4 of BUILTIN_THEME_NAMES).
    let target = crate::theme::BUILTIN_THEME_NAMES.iter().position(|n| *n == "yellow-storm").unwrap();
    let (state, _) = update(state, Command::ThemePickerMove(target as isize));
    let (state, effects) = update(state, Command::ThemePickerConfirm);
    assert!(state.theme_picker().is_none(), "the dialog closes");
    assert_eq!(state.theme.name, "yellow-storm", "the active theme switches in this same reducer step");
    assert_eq!(effects, vec![Effect::PersistTheme("yellow-storm".to_string())]);
}

#[test]
fn theme_picker_esc_changes_nothing() {
    let state = test_state(UiPhase::Panels); // active theme is nc-classic
    let (state, _) = update(state, Command::ThemePickerOpen);
    let (state, _) = update(state, Command::ThemePickerMove(3)); // highlight some other theme
    let (state, effects) = update(state, Command::ThemePickerCancel);
    assert!(state.theme_picker().is_none(), "the dialog closes");
    assert_eq!(state.theme.name, "nc-classic", "the active theme is untouched");
    assert!(effects.is_empty(), "nothing is persisted on cancel");
}

// ---------------------------------------------------------------------
// theme-picker-live-preview: State::render_theme()
// ---------------------------------------------------------------------

#[test]
fn render_theme_before_opening_the_picker_equals_the_active_theme() {
    let mut state = test_state(UiPhase::Panels);
    state.theme = Theme::terminal_green();
    assert_eq!(state.render_theme(), state.theme, "no picker open: render theme is the active theme");
}

#[test]
fn render_theme_while_open_tracks_the_highlighted_theme_as_it_moves() {
    let mut state = test_state(UiPhase::Panels); // active theme is nc-classic (index 0)
    state.theme = Theme::classic();
    let (state, _) = update(state, Command::ThemePickerOpen);
    // Opening is visually a no-op: highlight starts on the active theme.
    assert_eq!(state.render_theme(), state.theme, "opening previews the active theme first");

    let target = crate::theme::BUILTIN_THEME_NAMES.iter().position(|n| *n == "purple-lights").unwrap();
    let (state, _) = update(state, Command::ThemePickerMove(target as isize));
    assert_eq!(state.render_theme().name, "purple-lights", "moving the highlight previews the newly highlighted theme");
    assert_eq!(state.theme.name, "nc-classic", "the applied theme is untouched by moving the highlight");
}

#[test]
fn render_theme_after_cancel_reverts_to_the_active_theme() {
    let state = test_state(UiPhase::Panels); // active theme is nc-classic
    let (state, _) = update(state, Command::ThemePickerOpen);
    let (state, _) = update(state, Command::ThemePickerMove(3)); // preview some other theme
    assert_ne!(state.render_theme().name, "nc-classic", "sanity: a non-active theme is previewed");
    let (state, _) = update(state, Command::ThemePickerCancel);
    assert_eq!(state.render_theme(), state.theme, "after cancel, render theme is the (unchanged) active theme");
    assert_eq!(state.theme.name, "nc-classic", "cancel never touched the active theme");
}

#[test]
fn render_theme_after_confirm_equals_the_newly_applied_theme() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::ThemePickerOpen);
    let target = crate::theme::BUILTIN_THEME_NAMES.iter().position(|n| *n == "yellow-storm").unwrap();
    let (state, _) = update(state, Command::ThemePickerMove(target as isize));
    let (state, _) = update(state, Command::ThemePickerConfirm);
    assert_eq!(state.render_theme(), state.theme, "after confirm, render theme is the applied theme");
    assert_eq!(state.render_theme().name, "yellow-storm");
}

// ---------------------------------------------------------------------
// M5: F1 Help window + About dialog
// ---------------------------------------------------------------------

#[test]
fn help_opens_with_about_filecommand_highlighted_first() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::HelpOpen);
    let help = state.help().unwrap();
    assert_eq!(help.cursor, 0);
    assert_eq!(crate::dialogs::HELP_TOPICS[help.cursor], "About FileCommand");
    assert!(help.page.is_none());
    assert!(!help.about_open);
}

#[test]
fn help_activate_on_about_opens_the_about_dialog_not_a_page() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::HelpOpen);
    let (state, _) = update(state, Command::HelpActivate);
    assert!(state.help().unwrap().about_open);
    assert!(state.help().unwrap().page.is_none());
}

#[test]
fn help_activate_on_a_topic_opens_its_page() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::HelpOpen);
    let (state, _) = update(state, Command::HelpMove(1)); // "Keyboard reference"
    let (state, _) = update(state, Command::HelpActivate);
    assert_eq!(state.help().unwrap().page, Some(1));
}

#[test]
fn help_cancel_returns_a_page_to_the_list_then_closes_the_window() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::HelpOpen);
    let (state, _) = update(state, Command::HelpMove(1));
    let (state, _) = update(state, Command::HelpActivate);
    assert!(state.help().unwrap().page.is_some());

    let (state, _) = update(state, Command::HelpCancel);
    assert!(state.help().is_some(), "Esc from a page returns to the list, not closing the window");
    assert!(state.help().unwrap().page.is_none());
    assert_eq!(state.help().unwrap().cursor, 1, "the highlight is preserved");

    let (state, _) = update(state, Command::HelpCancel);
    assert!(state.help().is_none(), "Esc from the list closes the window");
}

#[test]
fn help_cancel_dismisses_about_back_to_the_list_with_about_still_highlighted() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::HelpOpen);
    let (state, _) = update(state, Command::HelpActivate); // opens About
    let (state, _) = update(state, Command::HelpCancel);
    assert!(state.help().is_some());
    assert!(!state.help().unwrap().about_open);
    assert_eq!(state.help().unwrap().cursor, 0);
}

#[test]
fn help_move_does_not_walk_past_the_ends_of_the_topic_list() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::HelpOpen);
    let (state, _) = update(state, Command::HelpMove(-5));
    assert_eq!(state.help().unwrap().cursor, 0);
    let last = crate::dialogs::HELP_TOPICS.len() - 1;
    let state = (0..20).fold(state, |s, _| update(s, Command::HelpMove(1)).0);
    assert_eq!(state.help().unwrap().cursor, last);
}

// ---------------------------------------------------------------------
// M5 review fix: startup-warning modal (malformed usermenu.toml)
// ---------------------------------------------------------------------

#[test]
fn dismiss_startup_warning_clears_it() {
    let mut state = test_state(UiPhase::Panels);
    state.set_startup_warning("usermenu.toml is malformed; F2 uses the default user menu".to_string());
    let (state, effects) = update(state, Command::DismissStartupWarning);
    assert_eq!(state.startup_warning(), None);
    assert!(effects.is_empty());
}

#[test]
fn dismiss_startup_warning_is_a_no_op_when_nothing_is_warned() {
    let state = test_state(UiPhase::Panels);
    assert_eq!(state.startup_warning(), None);
    let (state, effects) = update(state, Command::DismissStartupWarning);
    assert_eq!(state.startup_warning(), None);
    assert!(effects.is_empty());
}

// ---------------------------------------------------------------------
// File-action-menu row clicks (file-action-menu-mouse-click; mouse-input
// "File-action menu entries are clickable")
// ---------------------------------------------------------------------

/// Clicking a row activates it exactly like `Command::FileActionMenuConfirm`
/// would once the highlight is there —
/// `menu_item_click_activates_the_item_exactly_like_menu_activate`, mirrored
/// for the file-action menu.
#[test]
fn file_action_menu_item_click_activates_the_item_exactly_like_confirm() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let edit_index = state.file_action_menu().unwrap().entries.iter().position(|e| *e == FileActionMenuEntry::Edit).unwrap();
    let (state, effects) = update(state, Command::FileActionMenuItemClick(edit_index));
    assert!(state.file_action_menu().is_none(), "activating an entry closes the menu");
    assert_eq!(effects, vec![Effect::OpenEditor { request: 3, path: PathBuf::from("/left/notes.txt") }]);
}

#[test]
fn file_action_menu_item_click_with_no_menu_open_is_a_no_op() {
    let state = test_state(UiPhase::Panels);
    let before = state.clone();
    let (state, effects) = update(state, Command::FileActionMenuItemClick(0));
    assert!(effects.is_empty());
    assert_eq!(state, before);
}

/// A stale/out-of-range index (shouldn't normally happen given hit-testing,
/// but guarded anyway) leaves the menu untouched.
#[test]
fn file_action_menu_item_click_out_of_range_is_a_no_op() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt");
    let before = state.clone();
    let (state, effects) = update(state, Command::FileActionMenuItemClick(999));
    assert!(effects.is_empty());
    assert_eq!(state, before, "an out-of-range index leaves the menu untouched");
}

/// Clicking a row other than the highlighted one activates the clicked row,
/// not the previously-highlighted one (file-action-menu-mouse-click
/// "Clicking a different row than the highlighted one activates the clicked
/// row").
#[test]
fn file_action_menu_item_click_on_a_non_highlighted_row_activates_the_clicked_row() {
    let state = opened_menu_state(&["notes.txt"], "notes.txt"); // Open highlighted first
    assert_eq!(state.file_action_menu().unwrap().selected(), FileActionMenuEntry::Open, "setup precondition");
    let delete_index = state.file_action_menu().unwrap().entries.iter().position(|e| *e == FileActionMenuEntry::Delete).unwrap();
    let (state, _) = update(state, Command::FileActionMenuItemClick(delete_index));
    assert!(state.file_action_menu().is_none());
    assert!(matches!(state.phase, UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { .. })), "Delete was activated, not Open");
}

// ---------------------------------------------------------------------
// ui-animations: the reducer only records *when* things opened
// ---------------------------------------------------------------------

/// A state whose clock reads `ms`, as if a `Tick(ms)` had just landed.
fn clocked(phase: UiPhase, ms: u64) -> State {
    State { clock_ms: ms, ..test_state(phase) }
}

#[test]
fn opening_the_bar_stamps_every_clock_from_the_reducer_clock() {
    let (state, _) = update(clocked(UiPhase::Panels, 1_000), Command::MenuOpen);
    let menu = state.menu().expect("the bar opens");
    assert_eq!(menu.opened_at_ms, 1_000);
    assert_eq!(menu.pulldown_opened_at_ms, 1_000);
    assert_eq!(menu.selected_at_ms, None, "opening is the unfold's job, not a highlight flash");
}

#[test]
fn stamps_come_from_the_last_tick_reading_not_a_live_clock() {
    // Same inputs, same outputs: the stamp depends on `clock_ms` alone.
    let a = update(clocked(UiPhase::Panels, 42), Command::MenuOpen).0;
    let b = update(clocked(UiPhase::Panels, 42), Command::MenuOpen).0;
    assert_eq!(a, b);
    let later = update(clocked(UiPhase::Panels, 43), Command::MenuOpen).0;
    assert_eq!(later.menu().unwrap().opened_at_ms, 43);
}

#[test]
fn horizontal_traversal_restamps_the_pulldown_but_not_the_bar() {
    let (state, _) = update(clocked(UiPhase::Panels, 1_000), Command::MenuOpen);
    let (state, _) = update(state, Command::Tick { now_ms: 1_040, epoch_ms: 1_040 });
    let (state, _) = update(state, Command::MenuNextMenu);
    let menu = state.menu().expect("still open");
    assert_eq!(menu.active, MenuId::Files);
    assert!(menu.pulldown_open, "traversal never shows a closed intermediate state");
    assert_eq!(menu.opened_at_ms, 1_000, "the bar itself did not reopen");
    assert_eq!(menu.pulldown_opened_at_ms, 1_040, "the adjacent pull-down unfolds afresh");
    assert_eq!(menu.selected_at_ms, None);
}

#[test]
fn hotkey_jump_and_title_click_restamp_the_pulldown_and_keep_the_bar_stamp() {
    let (state, _) = update(clocked(UiPhase::Panels, 1_000), Command::MenuOpen);
    let (state, _) = update(state, Command::Tick { now_ms: 1_030, epoch_ms: 1_030 });
    let (jumped, _) = update(state.clone(), Command::MenuHotkey('c'));
    let menu = jumped.menu().expect("open");
    assert_eq!(menu.active, MenuId::Commands);
    assert_eq!((menu.opened_at_ms, menu.pulldown_opened_at_ms), (1_000, 1_030));

    // A title click rebuilds the whole `MenuState`; the bar stamp must survive.
    let (clicked, _) = update(state, Command::MenuTitleClick(MenuId::Options));
    let menu = clicked.menu().expect("open");
    assert_eq!(menu.active, MenuId::Options);
    assert_eq!((menu.opened_at_ms, menu.pulldown_opened_at_ms), (1_000, 1_030));
}

#[test]
fn moving_the_highlight_stamps_only_the_highlight() {
    let (state, _) = update(clocked(UiPhase::Panels, 1_000), Command::MenuOpen);
    let (state, _) = update(state, Command::Tick { now_ms: 1_200, epoch_ms: 1_200 });
    let (state, _) = update(state, Command::MenuSelectNext);
    let menu = state.menu().cloned().expect("open");
    assert_eq!(menu.selected_at_ms, Some(1_200));
    assert_eq!((menu.opened_at_ms, menu.pulldown_opened_at_ms), (1_000, 1_000));

    // A later tick with no selection change leaves the stamp alone.
    let (state, _) = update(state, Command::Tick { now_ms: 1_500, epoch_ms: 1_500 });
    assert_eq!(state.menu().unwrap().selected_at_ms, Some(1_200));
}

#[test]
fn reopening_a_collapsed_pulldown_unfolds_it_again() {
    let (state, _) = update(clocked(UiPhase::Panels, 1_000), Command::MenuOpen);
    let (state, _) = update(state, Command::MenuCollapse);
    assert!(!state.menu().unwrap().pulldown_open);
    let (state, _) = update(state, Command::Tick { now_ms: 1_300, epoch_ms: 1_300 });
    // The hotkey of the *same* menu (`Left`) reopens its pull-down.
    let (state, _) = update(state, Command::MenuHotkey('l'));
    let menu = state.menu().expect("open");
    assert_eq!(menu.active, MenuId::Left);
    assert!(menu.pulldown_open, "the hotkey reopens the collapsed pull-down");
    assert_eq!(menu.pulldown_opened_at_ms, 1_300);
    assert_eq!(menu.opened_at_ms, 1_000, "the bar stayed open throughout");
}

#[test]
fn user_menu_stamps_open_and_highlight_moves() {
    let mut state = clocked(UiPhase::Panels, 2_000);
    state.user_menu_entries =
        vec![UserMenuEntry { label: "A".to_string(), command: "a".to_string() }, UserMenuEntry { label: "B".to_string(), command: "b".to_string() }];
    let (state, _) = update(state, Command::UserMenuOpen);
    let menu = state.user_menu().expect("opens");
    assert_eq!((menu.opened_at_ms, menu.selected_at_ms), (2_000, None));

    let (state, _) = update(State { clock_ms: 2_080, ..state }, Command::UserMenuMove(1));
    let menu = state.user_menu().expect("open");
    assert_eq!(menu.cursor, 1);
    assert_eq!((menu.opened_at_ms, menu.selected_at_ms), (2_000, Some(2_080)));
}

#[test]
fn file_action_menu_stamps_open_and_highlight_moves() {
    let mut state = clocked(UiPhase::Panels, 3_000);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::Enter);
    let menu = state.file_action_menu().cloned().expect("Enter on a file opens the action menu");
    assert_eq!((menu.opened_at_ms, menu.selected_at_ms), (3_000, None));

    let (state, _) = update(State { clock_ms: 3_060, ..state }, Command::FileActionMenuMove(1));
    let menu = state.file_action_menu().expect("open");
    assert_eq!((menu.opened_at_ms, menu.selected_at_ms), (3_000, Some(3_060)));
}

#[test]
fn dialogs_stamp_when_they_open() {
    let (state, _) = update(clocked(UiPhase::Panels, 500), Command::RequestMkdir);
    assert!(matches!(state.phase, UiPhase::FileOpSetup(_)));
    assert_eq!(state.dialog_opened_at_ms, 500);

    let (state, _) = update(clocked(UiPhase::Panels, 700), Command::HelpOpen);
    assert!(state.help().is_some());
    assert_eq!(state.dialog_opened_at_ms, 700);

    let (state, _) = update(clocked(UiPhase::Panels, 900), Command::FuzzyJumpOpen);
    assert!(state.fuzzy_jump().is_some());
    assert_eq!(state.dialog_opened_at_ms, 900);

    let (state, _) = update(clocked(UiPhase::Panels, 1_100), Command::ThemePickerOpen);
    assert!(state.theme_picker().is_some());
    assert_eq!(state.dialog_opened_at_ms, 1_100);
}

#[test]
fn typing_into_a_dialog_does_not_restamp_it() {
    let (state, _) = update(clocked(UiPhase::Panels, 500), Command::RequestMkdir);
    let (state, _) = update(State { clock_ms: 640, ..state }, Command::TextEdit(TextEditOp::Insert('x')));
    assert_eq!(state.dialog_opened_at_ms, 500, "same dialog, no new iris");
}

#[test]
fn a_replacement_dialog_restamps_but_progress_updates_do_not() {
    let state = State { clock_ms: 100, ..running_progress_state(JobKind::Copy, "/left", "/right") };
    let info = ProgressInfo { files_done: 1, files_total: 3, bytes_done: 10, bytes_total: 30, current_file: OsString::from("a.txt") };
    let (state, _) = update(State { clock_ms: 200, ..state }, Command::JobProgress(info));
    assert_eq!(state.dialog_opened_at_ms, 0, "progress counters ticking is not a new dialog");

    // Progress -> conflict: a different dialog in the same slot irises again.
    let (state, _) = update(State { clock_ms: 300, ..state }, Command::JobConflict(sample_conflict()));
    assert!(matches!(state.phase, UiPhase::FileOpRunning { dialog: RunningDialog::Conflict { .. }, .. }));
    assert_eq!(state.dialog_opened_at_ms, 300);
}

#[test]
fn closing_a_dialog_stamps_nothing() {
    let (state, _) = update(clocked(UiPhase::Panels, 700), Command::HelpOpen);
    let (state, _) = update(State { clock_ms: 900, ..state }, Command::HelpCancel);
    assert!(state.help().is_none());
    assert_eq!(state.dialog_opened_at_ms, 700);
}

#[test]
fn the_quit_dialog_has_its_own_stamp_and_never_restamps_the_dialog_beneath() {
    let (state, _) = update(clocked(UiPhase::Panels, 700), Command::HelpOpen);
    let (state, _) = update(State { clock_ms: 950, ..state }, Command::RequestQuit);
    assert!(state.quit_confirm);
    assert_eq!(state.quit_opened_at_ms, 950);
    assert_eq!(state.dialog_opened_at_ms, 700, "the dialog under the quit prompt does not re-animate");

    // Cancelling restores the context exactly and leaves both stamps alone.
    let (state, _) = update(State { clock_ms: 1_200, ..state }, Command::CancelQuit);
    assert!(!state.quit_confirm && state.help().is_some());
    assert_eq!((state.quit_opened_at_ms, state.dialog_opened_at_ms), (950, 700));
}

#[test]
fn leaving_the_splash_for_the_panels_stamps_the_reveal() {
    // By the hold elapsing on a Tick...
    let state = test_state(UiPhase::Splash { started_at_ms: 1_000 });
    let (state, _) = update(state, Command::Tick { now_ms: 1_799, epoch_ms: 1_799 });
    assert_eq!(state.panels_revealed_at_ms, None, "still on the splash");
    let (state, _) = update(state, Command::Tick { now_ms: 1_800, epoch_ms: 1_800 });
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(state.panels_revealed_at_ms, Some(1_800));

    // ...or by a dismissing key.
    let (state, _) = update(State { clock_ms: 1_250, ..test_state(UiPhase::Splash { started_at_ms: 1_000 }) }, Command::ToggleActivePanel);
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(state.panels_revealed_at_ms, Some(1_250));
}

#[test]
fn a_key_during_the_splash_still_dismisses_it_entirely_and_is_consumed() {
    // startup-splash "Key mid-animation dismisses entirely": with animations
    // on, a key 150 ms in goes straight to the panels and is not forwarded.
    let mut state = test_state(UiPhase::Splash { started_at_ms: 1_000 });
    state.animations = true;
    state.clock_ms = 1_150;
    let (state, effects) = update(state, Command::ToggleActivePanel);
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(state.active, PanelSide::Left, "the dismissing key was consumed, not applied");
    assert!(effects.is_empty());
}

#[test]
fn no_reveal_is_recorded_when_the_splash_never_showed() {
    // --nosplash / splash = false: frame 1 is the complete panel screen.
    let (state, _) = State::initial(Theme::classic(), (80, 24), 0, 0, PathBuf::from("/"), PathBuf::from("/"), false);
    assert_eq!(state.panels_revealed_at_ms, None);
    let (state, _) = update(state, Command::Tick { now_ms: 5_000, epoch_ms: 5_000 });
    assert_eq!(state.panels_revealed_at_ms, None, "later ticks never invent a reveal");

    // Below the floor the splash is skipped for the placeholder; growing
    // back to Panels is not a Splash -> Panels transition either.
    let (state, _) = State::initial(Theme::classic(), (40, 10), 0, 0, PathBuf::from("/"), PathBuf::from("/"), true);
    assert!(state.undersized);
    assert_eq!(state.phase, UiPhase::Panels);
    let (state, _) = update(state, Command::Resize(80, 24));
    assert!(!state.undersized);
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(state.panels_revealed_at_ms, None);
}

#[test]
fn leaving_the_splash_via_the_placeholder_records_no_reveal() {
    let state = test_state(UiPhase::Splash { started_at_ms: 0 });
    let (state, _) = update(state, Command::Resize(40, 10));
    let (state, _) = update(state, Command::Resize(80, 24));
    assert_eq!(state.phase, UiPhase::Panels);
    assert_eq!(state.panels_revealed_at_ms, None);
}

#[test]
fn input_during_an_animation_applies_to_the_final_state() {
    // ui-animations "Input during an animation applies to the final state":
    // 30 ms into the unfold, Down moves the selection exactly as it does
    // with animations disabled; the animation flag never changes a transition.
    let open = |animations: bool| {
        let mut state = clocked(UiPhase::Panels, 1_000);
        state.animations = animations;
        let (state, _) = update(state, Command::MenuOpen);
        let (state, _) = update(State { clock_ms: 1_030, ..state }, Command::MenuSelectNext);
        state
    };
    let (on, off) = (open(true), open(false));
    assert_eq!(on.menu().map(|m| (m.active, m.pulldown_open, m.selected)), off.menu().map(|m| (m.active, m.pulldown_open, m.selected)));
    assert_ne!(on.menu().unwrap().selected, MenuState::opened().selected, "Down really moved the highlight");
}

#[test]
fn animations_flag_is_off_in_empty_states_and_exposes_the_clock_only_when_on() {
    let mut state = State::empty(Theme::classic());
    state.clock_ms = 77;
    assert!(!state.animations, "States built without config never animate");
    assert_eq!(state.anim_now(), None);
    state.animations = true;
    assert_eq!(state.anim_now(), Some(77));
}
