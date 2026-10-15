use super::*;

// ---------------------------------------------------------------------
// Stale background tab refresh on activation (refresh-inactive-panel-on-
// delete; BIG-162)
// ---------------------------------------------------------------------

#[test]
fn switch_tab_to_stale_tab_issues_fresh_read_and_clears_the_flag() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.left.cwd = PathBuf::from("/left");
    // The listing had finished when the tab was left — a tab stashed
    // mid-listing is stale for a second reason (listing-request-guard).
    state.left.progress = crate::panel::ListingProgress::Complete { count: 0 };
    state.left.open_tab(); // tab 0 stashed at "/left" (background); tab 1 (active) also starts at "/left"
    state.left.begin_new_listing(PathBuf::from("/left/other")); // active tab moves elsewhere
    let (state, _) = listing_complete(state, PanelSide::Left, 0);
    let mut state = state;
    state.left.mark_background_tabs_stale(Path::new("/left"));
    assert!(state.left.tabs[0].stale);

    let (state, effects) = update(state, Command::SwitchTab(1));
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "activated the previously-stale background tab");
    assert_eq!(
        without_git_info_effects(effects),
        vec![start_listing(&state, PanelSide::Left, "/left")],
        "activating a stale tab must issue a fresh read"
    );
    // The fresh read finishes before the tab is left again.
    let (state, _) = listing_complete(state, PanelSide::Left, 0);

    // The flag must be consumed, not left set: switching away and back
    // must not trigger a second read.
    let (state, _) = update(state, Command::SwitchTab(2));
    assert_eq!(state.left.cwd, PathBuf::from("/left/other"));
    let (_, effects) = update(state, Command::SwitchTab(1));
    assert!(without_git_info_effects(effects).is_empty(), "the stale flag must not persist past its one consuming activation");
}

#[test]
fn close_tab_falling_back_to_stale_neighbor_issues_fresh_read() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.left.cwd = PathBuf::from("/left");
    state.left.open_tab(); // tab 0 stashed at "/left" (background); tab 1 (active) also starts at "/left"
    state.left.begin_new_listing(PathBuf::from("/left/other")); // active tab moves elsewhere
    state.left.mark_background_tabs_stale(Path::new("/left"));

    let (state, effects) = update(state, Command::CloseTab(PanelSide::Left));
    assert_eq!(state.left.tab_count(), 1, "closed the active tab, falling back to the one remaining tab");
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "fell back to the previously-stale background tab");
    assert_eq!(
        without_git_info_effects(effects),
        vec![start_listing(&state, PanelSide::Left, "/left")],
        "falling back to a stale neighbor must issue a fresh read"
    );
}

#[test]
fn switch_tab_to_non_stale_tab_issues_no_listing_effects() {
    // panel-tabs "Switching to a tab with no pending staleness is
    // unchanged": untouched by any job *and* fully listed when it was left.
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.left.cwd = PathBuf::from("/left");
    state.left.progress = crate::panel::ListingProgress::Complete { count: 0 };
    state.left.open_tab();
    state.left.begin_new_listing(PathBuf::from("/left/other"));
    // No mark_background_tabs_stale call: tab 0 is not stale.
    assert!(!state.left.tabs[0].stale, "a tab left with a complete listing is not stale");

    let (state, effects) = update(state, Command::SwitchTab(1));
    assert_eq!(state.left.cwd, PathBuf::from("/left"));
    assert!(effects.iter().all(|e| !matches!(e, Effect::StartListing { .. })), "no listing at all is started, got {effects:?}");
    assert!(without_git_info_effects(effects).is_empty(), "switching to a tab with no pending staleness must not re-read (unchanged behavior)");
}

#[test]
fn stale_tab_refresh_records_no_history_and_persists_nothing() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.left.cwd = PathBuf::from("/left");
    state.left.open_tab(); // tab 0 stashed at "/left" (background); tab 1 (active) also starts at "/left"
    state.left.begin_new_listing(PathBuf::from("/left/other")); // active tab moves elsewhere
    state.left.mark_background_tabs_stale(Path::new("/left"));
    let history_before = state.dir_history.clone();

    let (state, effects) = update(state, Command::SwitchTab(1));
    assert!(effects.iter().any(|e| matches!(e, Effect::StartListing { panel: PanelSide::Left, .. })), "the fresh read still happens");
    assert!(effects.iter().all(|e| !matches!(e, Effect::PersistHistory(_))), "a stale-activation refresh persists nothing, got {effects:?}");
    assert_eq!(state.dir_history, history_before, "a stale-activation refresh records no fuzzy-jump visit — the user switched tabs, not directories");
}

#[test]
fn close_tab_stale_neighbor_refresh_records_no_history_and_persists_nothing() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.left.cwd = PathBuf::from("/left");
    state.left.open_tab();
    state.left.begin_new_listing(PathBuf::from("/left/other"));
    state.left.mark_background_tabs_stale(Path::new("/left"));
    let history_before = state.dir_history.clone();

    let (state, effects) = update(state, Command::CloseTab(PanelSide::Left));
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "fell back to the stale neighbor");
    assert!(effects.iter().any(|e| matches!(e, Effect::StartListing { panel: PanelSide::Left, .. })), "the fresh read still happens");
    assert!(effects.iter().all(|e| !matches!(e, Effect::PersistHistory(_))), "a stale-activation refresh persists nothing, got {effects:?}");
    assert_eq!(state.dir_history, history_before, "a stale-activation refresh records no fuzzy-jump visit");
}

#[test]
fn history_up_recalls_previous_commands_newest_first() {
    let mut state = test_state(UiPhase::Panels);
    state.history = vec!["first".to_string(), "second".to_string(), "third".to_string()];
    let state = type_line(state, "x");

    let (state, _) = update(state, Command::CommandLineHistoryPrev);
    assert_eq!(state.command_line.text(), "third");
    let (state, _) = update(state, Command::CommandLineHistoryPrev);
    assert_eq!(state.command_line.text(), "second");
    let (state, _) = update(state, Command::CommandLineHistoryNext);
    assert_eq!(state.command_line.text(), "third");
}

#[test]
fn history_up_stops_at_the_oldest_entry() {
    let mut state = test_state(UiPhase::Panels);
    state.history = vec!["only".to_string()];
    let state = type_line(state, "x");
    let (state, _) = update(state, Command::CommandLineHistoryPrev);
    let (state, _) = update(state, Command::CommandLineHistoryPrev);
    assert_eq!(state.command_line.text(), "only");
}

#[test]
fn history_navigation_never_moves_the_panel_cursor() {
    let mut state = test_state(UiPhase::Panels);
    state.history = vec!["dir".to_string()];
    state.left.entries = vec![file_entry("a", 1), file_entry("b", 2)];
    state.left.cursor = 1;
    let state = type_line(state, "x");
    let (state, _) = update(state, Command::CommandLineHistoryPrev);
    assert_eq!(state.left.cursor, 1);
}

#[test]
fn history_recall_with_empty_history_is_a_noop() {
    let state = type_line(test_state(UiPhase::Panels), "x");
    let (state, effects) = update(state, Command::CommandLineHistoryPrev);
    assert_eq!(state.command_line.text(), "x");
    assert!(effects.is_empty());
}

// ---------------------------------------------------------------------
// M5: git_info — generation-key staleness guard (design D3; git-info
// "Query re-issued on navigation", "Silent absence on timeout and
// stale-result discarding")
// ---------------------------------------------------------------------

#[test]
fn initial_state_issues_a_git_info_query_for_each_panel() {
    let (state, effects) = State::initial(Theme::classic(), (80, 24), 0, 0, PathBuf::from("/l"), PathBuf::from("/r"), false);
    let left_request = state.left.git_request.expect("left panel mints a git-info request on startup");
    let right_request = state.right.git_request.expect("right panel mints a git-info request on startup");
    assert_ne!(left_request, right_request);
    assert!(effects.contains(&Effect::QueryGitInfo { panel: PanelSide::Left, path: PathBuf::from("/l"), request: left_request }));
    assert!(effects.contains(&Effect::QueryGitInfo { panel: PanelSide::Right, path: PathBuf::from("/r"), request: right_request }));
}

#[test]
fn navigating_reissues_the_git_info_query_and_clears_the_previous_directorys_info() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    state.left.git_info = crate::git_info::GitInfo { branch: Some("main".to_string()), ..Default::default() };
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, PathBuf::from("/left/sub"));
    assert_eq!(state.left.git_info, crate::git_info::GitInfo::none(), "the previous directory's git info is cleared, not carried over");
    let request = state.left.git_request.expect("navigating mints a fresh git-info request");
    assert!(effects.contains(&Effect::QueryGitInfo { panel: PanelSide::Left, path: PathBuf::from("/left/sub"), request }));
}

#[test]
fn a_reread_mints_a_fresh_git_info_request_even_for_the_same_path() {
    let state = test_state(UiPhase::Panels);
    let first_request = state.left.git_request;
    let (state, effects) = update(state, Command::RereadPanel(PanelSide::Left));
    let second_request = state.left.git_request.expect("re-reading mints a request id");
    assert_ne!(Some(second_request), first_request);
    assert!(effects.contains(&Effect::QueryGitInfo { panel: PanelSide::Left, path: PathBuf::from("/left"), request: second_request }));
}

#[test]
fn a_resolved_git_info_result_is_applied_in_place() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    let request = state.left.git_request.unwrap();
    let info = crate::git_info::GitInfo {
        branch: Some("main".to_string()),
        statuses: std::collections::HashMap::from([(OsString::from("a.txt"), crate::git_info::FileStatus::Modified)]),
    };
    let (state, effects) = update(
        state,
        Command::GitInfoResolved { panel: PanelSide::Left, path: PathBuf::from("/left"), request, info: info.clone(), timed_out_root: None },
    );
    assert!(effects.is_empty());
    assert_eq!(state.left.git_info, info);
}

#[test]
fn a_git_info_result_for_a_directory_the_panel_left_is_discarded() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    let request = state.left.git_request.unwrap();
    let info = crate::git_info::GitInfo { branch: Some("main".to_string()), ..Default::default() };
    let (state, _) =
        update(state, Command::GitInfoResolved { panel: PanelSide::Left, path: PathBuf::from("/elsewhere"), request, info, timed_out_root: None });
    assert_eq!(state.left.git_info, crate::git_info::GitInfo::none(), "a result for another directory is dropped");
}

#[test]
fn a_stale_git_info_result_from_an_out_of_order_reread_is_discarded_but_the_fresher_one_applies() {
    // Mirrors `a_stale_info_result_from_an_out_of_order_reread_is_discarded_but_the_fresher_one_applies`:
    // two RereadPanel commands for the same path mint two different
    // request ids, so `path` equality alone can't tell the stale reply
    // apart from the current one — this is also how a timed-out query's
    // late reply is safely dropped (git-info "Silent absence on timeout
    // and stale-result discarding").
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    let first_request = state.left.git_request.unwrap();

    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    let second_request = state.left.git_request.unwrap();
    assert_ne!(first_request, second_request, "re-reading mints a fresh request id even for the same path");

    let stale_info = crate::git_info::GitInfo { branch: Some("stale-branch".to_string()), ..Default::default() };
    let (state, _) = update(
        state,
        Command::GitInfoResolved {
            panel: PanelSide::Left,
            path: PathBuf::from("/left"),
            request: first_request,
            info: stale_info,
            timed_out_root: None,
        },
    );
    assert_eq!(state.left.git_info, crate::git_info::GitInfo::none(), "the stale (first-request) answer is dropped");

    let fresh_info = crate::git_info::GitInfo { branch: Some("main".to_string()), ..Default::default() };
    let (state, _) = update(
        state,
        Command::GitInfoResolved {
            panel: PanelSide::Left,
            path: PathBuf::from("/left"),
            request: second_request,
            info: fresh_info.clone(),
            timed_out_root: None,
        },
    );
    assert_eq!(state.left.git_info, fresh_info, "the current request's answer applies");
}

#[test]
fn a_timed_out_query_answered_late_with_no_info_is_silently_dropped_once_superseded() {
    // A worker-thread timeout is, from the reducer's point of view, just
    // another reply — it degrades to `GitInfo::none()` — so it goes
    // through the exact same generation-key guard as any other late
    // reply: once a fresher request is outstanding, the timed-out query's
    // eventual answer (of any content) must not clobber it.
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    let timed_out_request = state.left.git_request.unwrap();

    let (mut state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    let current_request = state.left.git_request.unwrap();
    state.left.git_info = crate::git_info::GitInfo { branch: Some("main".to_string()), ..Default::default() };

    let (state, _) = update(
        state,
        Command::GitInfoResolved {
            panel: PanelSide::Left,
            path: PathBuf::from("/left"),
            request: timed_out_request,
            info: GitInfo::none(),
            timed_out_root: None,
        },
    );
    assert_eq!(
        state.left.git_info.branch.as_deref(),
        Some("main"),
        "the timed-out query's late reply must not overwrite the current request's already-applied result"
    );
    let _ = current_request;
}

// ---------------------------------------------------------------------
// git-info-timeout-memory: a timed-out query marks its repository "no
// info" for the rest of the session (git-info "Silent absence on timeout
// and stale-result discarding"; design D1)
// ---------------------------------------------------------------------

/// The spec scenarios spell their repositories with Windows paths
/// (`D:\mono`); the same shapes are spelled `/mono` on Unix so the
/// component-wise prefix checks exercise identically on both.
#[cfg(windows)]
fn mono_path(sub: &str) -> PathBuf {
    let mut p = PathBuf::from(r"D:\mono");
    if !sub.is_empty() {
        p.push(sub);
    }
    p
}

/// Unix spelling of the spec scenarios' repository (see above).
#[cfg(not(windows))]
fn mono_path(sub: &str) -> PathBuf {
    let mut p = PathBuf::from("/mono");
    if !sub.is_empty() {
        p.push(sub);
    }
    p
}

/// A directory inside a *different* repository (`C:\work`, or `/work`).
#[cfg(windows)]
fn work_path(sub: &str) -> PathBuf {
    let mut p = PathBuf::from(r"C:\work");
    if !sub.is_empty() {
        p.push(sub);
    }
    p
}

/// Unix spelling of the other repository (see above).
#[cfg(not(windows))]
fn work_path(sub: &str) -> PathBuf {
    let mut p = PathBuf::from("/work");
    if !sub.is_empty() {
        p.push(sub);
    }
    p
}

/// Feed a timed-out reply for `path` with root `root` — the worker's report
/// when a query exceeded its timeout (design D2).
fn report_timeout(state: State, panel: PanelSide, path: PathBuf, root: PathBuf) -> State {
    let (state, effects) = update(state, Command::GitInfoResolved { panel, path, request: 1, info: GitInfo::none(), timed_out_root: Some(root) });
    assert!(effects.is_empty(), "recording a timeout emits no effects");
    state
}

#[test]
fn a_timed_out_repository_is_not_queried_again_for_the_session() {
    // git-info "Timed-out repository is not queried again" + the narrowed
    // "Query re-issued on navigation": once the query for `D:\mono\src`
    // times out, navigating to `D:\mono\docs` and re-reading there issue no
    // git query and leave the panel without git info.
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = mono_path("src");
    let mut state = report_timeout(state, PanelSide::Left, mono_path("src"), mono_path(""));
    assert_eq!(state.git_no_info_roots, vec![mono_path("")]);

    // Navigation into another directory of the same repository ...
    state.left.cwd = mono_path("");
    state.left.entries = vec![dir_entry("docs")];
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, mono_path("docs"));
    assert!(!effects.iter().any(|e| matches!(e, Effect::QueryGitInfo { .. })), "no query under a remembered root: {effects:?}");
    assert_eq!(state.left.git_info, GitInfo::none());
    assert_eq!(state.left.git_request, None, "no request is minted for a suppressed query");

    // ... and a re-read there neither.
    let (state, effects) = update(state, Command::RereadPanel(PanelSide::Left));
    assert!(!effects.iter().any(|e| matches!(e, Effect::QueryGitInfo { .. })), "no query on re-read either: {effects:?}");
    assert_eq!(state.left.git_info, GitInfo::none());
}

#[test]
fn other_repositories_are_still_queried_after_a_timeout_elsewhere() {
    // git-info "Other repositories are still queried".
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = mono_path("src");
    let mut state = report_timeout(state, PanelSide::Left, mono_path("src"), mono_path(""));

    state.left.cwd = work_path("");
    state.left.entries = vec![dir_entry("app")];
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, work_path("app"));
    let request = state.left.git_request.expect("a query is minted for the other repository");
    assert!(effects.contains(&Effect::QueryGitInfo { panel: PanelSide::Left, path: work_path("app"), request }));
}

#[test]
fn a_stale_timeout_reply_still_records_the_root() {
    // git-info "Timeout recorded after the panel moved on" + "Late result
    // after navigation is dropped": the panel has already left `D:\mono\src`
    // when the timeout reply arrives; the root is recorded anyway and the
    // current directory's rendering state is untouched.
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = work_path("app");
    state.left.git_info = GitInfo { branch: Some("main".to_string()), ..Default::default() };
    state.left.git_request = Some(9); // the fresher, still-outstanding request
    let (state, effects) = update(
        state,
        Command::GitInfoResolved {
            panel: PanelSide::Left,
            path: mono_path("src"),
            request: 4, // the superseded, timed-out request
            info: GitInfo::none(),
            timed_out_root: Some(mono_path("")),
        },
    );
    assert!(effects.is_empty());
    assert_eq!(state.git_no_info_roots, vec![mono_path("")], "the root is recorded even though the reply is stale");
    assert_eq!(state.left.git_info.branch.as_deref(), Some("main"), "the current panel is unchanged");
    assert_eq!(state.left.git_request, Some(9));

    // And the recorded root has teeth: a later navigation into it starts no
    // query.
    let mut state = state;
    state.left.cwd = mono_path("");
    state.left.entries = vec![dir_entry("docs")];
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, mono_path("docs"));
    assert!(!effects.iter().any(|e| matches!(e, Effect::QueryGitInfo { .. })), "{effects:?}");
}

#[test]
fn a_timeout_before_the_repository_was_located_marks_only_the_queried_directory() {
    // git-info "Repository not located before the timeout": the queried
    // directory itself is the remembered root, so its descendants are
    // covered but its parent (and siblings) are not.
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = work_path("proj");
    let mut state = report_timeout(state, PanelSide::Left, work_path("proj"), work_path("proj"));

    // A descendant of the queried directory: no query.
    state.left.entries = vec![dir_entry("sub")];
    let (mut state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, work_path("proj/sub"));
    assert!(!effects.iter().any(|e| matches!(e, Effect::QueryGitInfo { .. })), "{effects:?}");

    // The parent directory is outside the remembered root: still queried.
    state.left.cwd = work_path("proj");
    state.left.entries = vec![Entry::parent_dir()];
    state.left.cursor = 0;
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, work_path(""));
    let request = state.left.git_request.expect("the parent is still queried");
    assert!(effects.contains(&Effect::QueryGitInfo { panel: PanelSide::Left, path: work_path(""), request }));
}

#[test]
fn swapping_a_panel_into_a_timed_out_repository_issues_no_query_and_clears_its_info() {
    // git-info "Swapping a panel into a timed-out repository": the right
    // panel resolved branch info inside `D:\mono` before the timeout;
    // Ctrl+U must not let it keep showing that info, and must not start a
    // query for it.
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = work_path("app");
    state.right.cwd = mono_path("lib");
    state.right.git_info = GitInfo { branch: Some("main".to_string()), ..Default::default() };
    state.right.git_request = Some(12);
    complete_panel_listings(&mut state);
    let state = report_timeout(state, PanelSide::Left, mono_path("src"), mono_path(""));

    let (state, effects) = update(state, Command::SwapPanels);
    // The panel now showing `D:\mono\lib` (the left panel after the swap)
    // gets no query and shows no git info ...
    assert_eq!(state.left.cwd, mono_path("lib"));
    assert_eq!(state.left.git_info, GitInfo::none(), "branch info resolved before the timeout is cleared");
    assert_eq!(state.left.git_request, None);
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::QueryGitInfo { panel, path, .. } if *panel == PanelSide::Left || *path == mono_path("lib"))),
        "no query for the panel inside the timed-out repository: {effects:?}"
    );
    // ... while the other panel, outside it, is queried as usual.
    let request = state.right.git_request.expect("the other panel is re-queried after the swap");
    assert!(effects.contains(&Effect::QueryGitInfo { panel: PanelSide::Right, path: work_path("app"), request }));
}

#[test]
fn a_second_timeout_at_a_covered_root_is_not_recorded_twice() {
    // Two panels can have a query in flight in the same repository; both
    // may time out. The set stays deduplicated.
    let state = test_state(UiPhase::Panels);
    let state = report_timeout(state, PanelSide::Left, mono_path("src"), mono_path(""));
    let state = report_timeout(state, PanelSide::Right, mono_path("docs"), mono_path(""));
    assert_eq!(state.git_no_info_roots, vec![mono_path("")]);
    let state = report_timeout(state, PanelSide::Right, mono_path("docs"), mono_path("docs"));
    assert_eq!(state.git_no_info_roots, vec![mono_path("")], "a root under a remembered root is already covered");
}

// ---------------------------------------------------------------------
// M5: directory frecency recording + persistence (fuzzy-jump "Navigation
// records history")
// ---------------------------------------------------------------------

#[test]
fn navigating_into_a_directory_records_and_persists_frecency() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_entry("sub")];
    // Visits are stamped with the wall-clock reading (history-persistence-
    // hardening design D1), not the monotonic one.
    state.epoch_ms = 5_000;
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.dir_history, vec![history_entry("/left/sub", 1, 5_000)]);
    assert!(effects.iter().any(|e| matches!(e, Effect::PersistHistory(file) if file.directories == state.dir_history)));
}

#[test]
fn revisiting_a_directory_increments_its_frecency_entry_rather_than_duplicating() {
    let mut state = test_state(UiPhase::Panels);
    state.dir_history = vec![history_entry(r"C:\a", 1, 0)];
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left)); // /left, a fresh entry
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left)); // /left again
    let count = state.dir_history.iter().find(|e| e.path.as_path() == Path::new("/left")).unwrap().visit_count;
    assert_eq!(count, 2, "re-reading the same directory increments the same entry rather than adding a duplicate");
}

#[test]
fn tree_cursor_preview_of_the_opposite_panel_does_not_record_frecency() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = update(state, Command::EnterTreeMode(PanelSide::Left));
    let (state, _) =
        update(state, Command::TreeNodeExpanded { panel: PanelSide::Left, path: PathBuf::from(r"C:\"), children: vec![dir_child("alpha")] });
    let (state, _) = update(state, Command::MoveCursor(CursorMove::Down(1)));
    assert!(state.dir_history.is_empty(), "browsing the tree previews the opposite panel; it does not count as a visit");
}

// ---------------------------------------------------------------------
// Panel-scrolling: reducer-level viewport reconciliation (task 1.5)
//
// `panel::tests::scroll_offset_tests` exercises `ensure_cursor_visible`
// directly at fixed row counts; these tests instead drive the reducer end
// to end so each wiring site in `update.rs` (task 1.4) is proven to
// actually call it, at term sizes chosen so `panel_viewport_rows` produces
// a known, hand-verifiable row count (visible here via `super::*`, the same
// private helper `update`'s own reducer arms use).
// ---------------------------------------------------------------------

#[test]
fn full_mode_viewport_rows_at_80x24_are_19() {
    // Every test below is built on this figure: panels_h = 24 - 2 = 22, no
    // tab strip at 1 tab (reserved = 2), body_h = 20, Full/Tree = body_h -
    // 1 (header row) = 19.
    assert_eq!(panel_viewport_rows((80, 24), DisplayMode::Full, 1), 19);
}

#[test]
fn quick_filter_narrowing_re_clamps_the_offset_through_the_reducer() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Full-mode body: 19 rows
    state.left.sort_mode = SortMode::Unsorted;
    state.left.entries = (0..30).map(|i| file_entry(&format!("e{i}"), 0)).collect();
    state.left.cursor = 25; // "e25" -- one of only three entries containing "5"
    state.left.scroll_offset = 15; // a stale offset from before filtering narrowed the list

    let (state, _) = update(state, Command::QuickFilterStart(PanelSide::Left));
    let (state, _) = update(state, Command::QuickFilterChar('5'));

    let visible: Vec<String> = state.left.visible_indices().into_iter().map(|i| state.left.entries[i].name.to_string_lossy().into_owned()).collect();
    assert_eq!(visible, vec!["e5", "e15", "e25"], "only entries containing \"5\" remain");
    assert_eq!(state.left.cursor, 25, "\"e25\" stays selected — it's still visible under the filter");
    assert_eq!(state.left.scroll_offset, 2, "\"e25\" is now at visible position 2; the stale offset (15) must re-clamp to it (panel-navigation \"Quick-filter narrowing re-clamps the offset\")");
}

#[test]
fn re_sort_re_clamps_the_offset_through_the_reducer() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Full-mode body: 19 rows
    state.left.sort_mode = SortMode::Unsorted;
    // Descending by size, so "e00" (the largest) starts at position 0.
    state.left.entries = (0..30).map(|i| file_entry(&format!("e{i:02}"), 29 - i as u64)).collect();
    state.left.cursor = 0; // "e00"
    state.left.scroll_offset = 0;

    let (state, _) = update(state, Command::SetSortMode { side: PanelSide::Left, mode: SortMode::Size });

    assert_eq!(state.left.entries.last().unwrap().name, OsString::from("e00"), "ascending-by-size moves the largest entry to the very end");
    assert_eq!(state.left.cursor, 29, "the cursor re-anchors onto \"e00\"'s new position");
    assert_eq!(state.left.scroll_offset, 11, "29 + 1 - 19 = 11 (panel-navigation \"Re-sort keeps the cursor's entry in view\")");
}

#[test]
fn terminal_shrinking_re_clamps_the_offset_on_resize() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Full-mode body: 19 rows
    state.left.sort_mode = SortMode::Unsorted;
    state.left.entries = (0..30).map(|i| file_entry(&format!("e{i}"), 0)).collect();
    state.left.cursor = 18;
    state.left.scroll_offset = 0;

    let (state, _) = update(state, Command::Resize(80, 24));
    assert_eq!(state.left.scroll_offset, 0, "no shrink yet: the cursor is already inside the 19-row window");

    // 80x14: panels_h = 12, no tab strip, body_h = 10, Full rows = 9.
    assert_eq!(panel_viewport_rows((80, 14), DisplayMode::Full, 1), 9);
    let (state, _) = update(state, Command::Resize(80, 14));
    assert_eq!(state.left.scroll_offset, 10, "18 + 1 - 9 = 10 (panel-navigation \"Terminal resize re-clamps\")");
}

#[test]
fn tab_restore_re_clamps_against_the_current_viewport_through_the_reducer() {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.term_size = (80, 24); // Full-mode body: 19 rows
    state.left.sort_mode = SortMode::Unsorted;
    state.left.entries = (0..30).map(|i| file_entry(&format!("e{i}"), 0)).collect();
    state.left.cursor = 18;
    state.left.scroll_offset = 10; // valid at 19 rows: window [10, 29)
                                   // The listing had finished before the tab was stashed: a tab stashed
                                   // mid-listing is stale and re-read on activation (listing-request-guard),
                                   // which would reset the cursor this test is about.
    state.left.progress = crate::panel::ListingProgress::Complete { count: 30 };

    // Stash this as tab 1; the new (now-active) tab inherits the same
    // cursor/offset and is left untouched from here on.
    let (state, _) = update(state, Command::OpenTab(PanelSide::Left));
    assert_eq!(state.left.tab_count(), 2);

    // Shrink drastically enough that the stashed tab's offset (10) will no
    // longer keep its cursor (18) in view once restored.
    let (state, _) = update(state, Command::Resize(80, 10));
    // 80x10, 2 tabs: panels_h = 8, tab strip visible (reserved = 3),
    // body_h = 5, Full rows = 4.
    assert_eq!(panel_viewport_rows((80, 10), DisplayMode::Full, 2), 4);

    let (state, _) = update(state, Command::SwitchTab(1));
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "tab 1 is the originally-stashed tab");
    assert_eq!(state.left.cursor, 18, "the restored cursor position round-trips exactly");
    assert_eq!(
        state.left.scroll_offset, 15,
        "the stashed offset (10) no longer fits a 4-row window around cursor 18; 18 + 1 - 4 = 15 (panel-navigation \"Tab restore re-clamps against the current viewport\")"
    );
}

#[test]
fn streamed_chunk_keeps_the_offset_pinned_to_zero_via_the_reducer() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Full-mode body: 19 rows
    state.left.sort_mode = SortMode::Name;
    let entries: Vec<Entry> = (0..30).map(|i| file_entry(&format!("e{i:02}"), 0)).collect();

    let (state, _) = listing_chunk(state, PanelSide::Left, entries);

    assert_eq!(state.left.cursor, 0, "the cursor stays pinned to the top while the user hasn't moved it");
    assert_eq!(
        state.left.scroll_offset, 0,
        "the window stays pinned to the top right along with the cursor (panel-navigation \"Streamed listing keeps the top pinned\")"
    );
}

#[test]
fn find_file_settle_on_listing_complete_lands_the_cursor_in_view() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Full-mode body: 19 rows
    state.left.sort_mode = SortMode::Unsorted;
    state.left.pending_cursor_target = Some(OsString::from("e25"));
    let entries: Vec<Entry> = (0..30).map(|i| file_entry(&format!("e{i}"), 0)).collect();

    let (state, _) = listing_chunk(state, PanelSide::Left, entries);
    let (state, _) = listing_complete(state, PanelSide::Left, 30);

    assert_eq!(
        state.left.entries[state.left.cursor].name,
        OsString::from("e25"),
        "find-file's deferred settle lands the cursor on the matched entry"
    );
    assert_eq!(state.left.cursor, 25);
    assert_eq!(
        state.left.scroll_offset, 7,
        "25 + 1 - 19 = 7: the settled cursor lands inside the window (panel-navigation \"Scroll offset is core panel state\" -- find-file's deferred cursor settle re-clamps)"
    );
}

// ---------------------------------------------------------------------
// Same-directory re-read preserves cursor and scroll position
// (reread-preserves-cursor; panel-navigation "Same-directory re-read
// preserves cursor and scroll position")
// ---------------------------------------------------------------------

/// A panel mid-delete-job browsing `dir` with `names` as its completed
/// listing, mirroring `running_progress_state` but with the opposite panel
/// parked on an unaffected directory so only this one re-reads.
fn deleting_state(dir: &str, names: &[String], cursor: usize, scroll_offset: usize) -> State {
    let mut state = running_progress_state(JobKind::Delete, dir, dir);
    state.term_size = (80, 25); // Full-mode body: 20 rows (25 - 2 panels frame - 2 reserved - 1 header)
    assert_eq!(panel_viewport_rows((80, 25), DisplayMode::Full, 1), 20);
    state.right.cwd = PathBuf::from("/right"); // unaffected panel: must not be re-read
    let left = &mut state.left;
    left.cwd = PathBuf::from(dir);
    left.sort_mode = SortMode::Unsorted;
    left.entries = names.iter().map(|n| file_entry(n, 0)).collect();
    left.progress = crate::panel::ListingProgress::Complete { count: names.len() };
    left.cursor = cursor;
    left.cursor_user_moved = true;
    left.scroll_offset = scroll_offset;
    state
}

/// Finish the delete: `JobDone`, then stream the surviving `survivors` back
/// as the automatic re-read (single chunk + completion).
fn finish_delete(state: State, dir: &str, survivors: &[String]) -> State {
    let (state, effects) = update(
        state,
        Command::JobDone { outcome: JobOutcome::Completed { skipped: vec![] }, source_dir: PathBuf::from(dir), dest_dir: PathBuf::from(dir) },
    );
    assert_eq!(without_git_info_effects(effects), vec![start_listing(&state, PanelSide::Left, dir)], "the deleted-from panel re-reads");
    let entries: Vec<Entry> = survivors.iter().map(|n| file_entry(n, 0)).collect();
    let total = entries.len();
    let (state, _) = listing_chunk(state, PanelSide::Left, entries);
    let (state, _) = listing_complete(state, PanelSide::Left, total);
    state
}

fn names_of(prefix: &str, n: usize) -> Vec<String> {
    (0..n).map(|i| format!("{prefix}{i:02}")).collect()
}

#[test]
fn job_done_delete_of_the_cursored_entry_lands_on_the_following_entry() {
    // panel-navigation "Deleting the cursored file lands on the next entry";
    // file-operations "Post-delete re-read keeps the user's place".
    let names: Vec<String> = ["a.txt", "b.txt", "c.txt"].iter().map(|s| s.to_string()).collect();
    let state = deleting_state("/left", &names, 1, 0); // cursor on "b.txt"
    let survivors: Vec<String> = ["a.txt", "c.txt"].iter().map(|s| s.to_string()).collect();

    let state = finish_delete(state, "/left", &survivors);

    assert_eq!(state.left.entries[state.left.cursor].name, OsString::from("c.txt"), "the cursor lands on the entry that followed the deleted one");
    assert_eq!(state.left.cursor, 1);
    assert_eq!(state.left.scroll_offset, 0, "the scroll offset is unchanged");
    assert!(state.left.reread_anchor.is_none(), "the anchor is consumed on completion");
}

#[test]
fn job_done_delete_of_the_last_entry_lands_on_the_previous_one() {
    // panel-navigation "Deleting the last entry lands on the previous one".
    let names: Vec<String> = ["a.txt", "b.txt", "c.txt"].iter().map(|s| s.to_string()).collect();
    let state = deleting_state("/left", &names, 2, 0); // cursor on "c.txt", the last entry
    let survivors: Vec<String> = ["a.txt", "b.txt"].iter().map(|s| s.to_string()).collect();

    let state = finish_delete(state, "/left", &survivors);

    assert_eq!(
        state.left.entries[state.left.cursor].name,
        OsString::from("b.txt"),
        "the cursor lands on the entry that preceded the deleted one, now the last"
    );
    assert_eq!(state.left.cursor, 1);
}

#[test]
fn job_done_multi_selection_delete_lands_on_the_first_survivor_at_or_after_the_cursor() {
    // panel-navigation "Multi-selection delete lands on the first survivor
    // at or after the cursor": {b, c, d} selected with the cursor on c.
    let names: Vec<String> = ["a", "b", "c", "d", "e"].iter().map(|s| s.to_string()).collect();
    let mut state = deleting_state("/left", &names, 2, 0); // cursor on "c"
    for n in ["b", "c", "d"] {
        state.left.selected.insert(OsString::from(n));
    }
    let survivors: Vec<String> = ["a", "e"].iter().map(|s| s.to_string()).collect();

    let state = finish_delete(state, "/left", &survivors);

    assert_eq!(
        state.left.entries[state.left.cursor].name,
        OsString::from("e"),
        "b, c, and d are all gone; the first survivor at or after the cursor's old row is e"
    );
}

#[test]
fn job_done_delete_in_a_scrolled_window_keeps_the_window_where_it_was() {
    // panel-navigation "Scrolled window stays put": 60 entries in a 20-row
    // body, offset 30, cursor at position 40.
    let names = names_of("e", 60);
    let state = deleting_state("/left", &names, 40, 30);
    let survivors = names_of("e", 60).into_iter().filter(|n| n != "e40").collect::<Vec<_>>();

    let state = finish_delete(state, "/left", &survivors);

    assert_eq!(state.left.entries[state.left.cursor].name, OsString::from("e41"), "the cursor lands on the entry that followed the deleted one");
    assert_eq!(state.left.cursor, 40, "which is back at position 40 in the 59-entry list");
    assert_eq!(state.left.scroll_offset, 30, "the scrolled window stays put");
}

#[test]
fn job_done_delete_at_the_very_bottom_pulls_the_window_up_by_one() {
    // panel-navigation "Deleting at the very bottom pulls the window up by
    // one" and "Re-read anchor restore re-clamps against the shorter list".
    let names = names_of("e", 60);
    let state = deleting_state("/left", &names, 59, 40); // cursor on the last entry
    let survivors = names_of("e", 59);

    let state = finish_delete(state, "/left", &survivors);

    assert_eq!(state.left.entries.len(), 59);
    assert_eq!(state.left.entries[state.left.cursor].name, OsString::from("e58"), "the cursor lands on the new last entry");
    assert_eq!(state.left.cursor, 58);
    assert_eq!(state.left.scroll_offset, 39, "the offset re-clamps to 59 - 20 so no blank trailing row shows");
}

#[test]
fn reread_panel_keeps_the_cursors_entry_and_offset() {
    // panel-navigation "Ctrl+R keeps the cursor's entry".
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Full-mode body: 19 rows
    state.left.sort_mode = SortMode::Unsorted;
    let mut names = names_of("e", 30);
    names[15] = "report.txt".to_string();
    state.left.entries = names.iter().map(|n| file_entry(n, 0)).collect();
    state.left.progress = crate::panel::ListingProgress::Complete { count: 30 };
    state.left.cursor = 15; // "report.txt", partway down
    state.left.cursor_user_moved = true;
    state.left.scroll_offset = 5;

    let (state, effects) = update(state, Command::RereadPanel(PanelSide::Left));
    assert_eq!(without_git_info_effects(effects), vec![start_listing(&state, PanelSide::Left, "/left")]);
    assert!(state.left.reread_anchor.is_some(), "a same-directory re-read captures the anchor before resetting");

    // The unchanged directory streams back.
    let entries: Vec<Entry> = names.iter().map(|n| file_entry(n, 0)).collect();
    let (state, _) = listing_chunk(state, PanelSide::Left, entries);
    let (state, _) = listing_complete(state, PanelSide::Left, 30);

    assert_eq!(state.left.entries[state.left.cursor].name, OsString::from("report.txt"));
    assert_eq!(state.left.cursor, 15);
    assert_eq!(state.left.scroll_offset, 5, "the scroll offset is unchanged");
}

#[test]
fn the_first_chunk_containing_the_anchored_entry_already_places_the_cursor_on_it() {
    // panel-navigation "No jump while the re-read streams".
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Full-mode body: 19 rows
    state.left.sort_mode = SortMode::Unsorted;
    let names = names_of("e", 30);
    state.left.entries = names.iter().map(|n| file_entry(n, 0)).collect();
    state.left.progress = crate::panel::ListingProgress::Complete { count: 30 };
    state.left.cursor = 12; // "e12"
    state.left.cursor_user_moved = true;
    state.left.scroll_offset = 4;

    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));

    // First chunk: e00..e14 — it contains the anchored "e12".
    let first: Vec<Entry> = names[..15].iter().map(|n| file_entry(n, 0)).collect();
    let (state, _) = listing_chunk(state, PanelSide::Left, first);
    assert_eq!(
        state.left.entries[state.left.cursor].name,
        OsString::from("e12"),
        "the cursor is on the anchored entry as soon as its chunk lands, not on the first entry"
    );
    assert_ne!(state.left.cursor, 0);

    // The rest of the listing must not move it again.
    let rest: Vec<Entry> = names[15..].iter().map(|n| file_entry(n, 0)).collect();
    let (state, _) = listing_chunk(state, PanelSide::Left, rest);
    let (state, _) = listing_complete(state, PanelSide::Left, 30);
    assert_eq!(state.left.entries[state.left.cursor].name, OsString::from("e12"));
    assert_eq!(state.left.cursor, 12);
    assert_eq!(state.left.scroll_offset, 4, "the offset is restored once the whole list is back");
}

#[test]
fn navigating_into_a_subdirectory_after_a_reread_still_starts_at_the_top() {
    // panel-navigation "Navigating to a different directory still starts at
    // the top".
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24);
    state.left.sort_mode = SortMode::Unsorted;
    let mut entries: Vec<Entry> = names_of("e", 30).iter().map(|n| file_entry(n, 0)).collect();
    entries.push(dir_entry("sub"));
    state.left.entries = entries;
    state.left.progress = crate::panel::ListingProgress::Complete { count: 31 };
    state.left.cursor = 20;
    state.left.cursor_user_moved = true;
    state.left.scroll_offset = 10;

    // A re-read happens first (and its anchor is consumed on completion)...
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    let mut again: Vec<Entry> = names_of("e", 30).iter().map(|n| file_entry(n, 0)).collect();
    again.push(dir_entry("sub"));
    let (state, _) = listing_chunk(state, PanelSide::Left, again);
    let (state, _) = listing_complete(state, PanelSide::Left, 31);
    assert_eq!(state.left.scroll_offset, 10, "the re-read kept the user's place");

    // ...then Enter on "sub" navigates away: cursor and offset reset, and no
    // anchor rides along into the new directory.
    let mut state = state;
    state.left.cursor = state.left.entries.iter().position(|e| e.name == "sub").unwrap();
    let (state, _) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, PathBuf::from("/left/sub"));
    assert_eq!(state.left.cursor, 0);
    assert_eq!(state.left.scroll_offset, 0);
    assert!(state.left.reread_anchor.is_none(), "navigation to a different directory clears any anchor");

    // The streamed fresh listing stays pinned to the top.
    let (state, _) = listing_chunk(state, PanelSide::Left, vec![file_entry("x", 1), file_entry("y", 2)]);
    assert_eq!(state.left.cursor, 0);
    assert_eq!(state.left.scroll_offset, 0);
}

#[test]
fn reread_issued_while_the_previous_listing_is_still_streaming_captures_no_anchor() {
    // reread-preserves-cursor design D5: anchoring to a half-loaded list is
    // worse than keeping the old jump-to-top behavior.
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24);
    state.left.sort_mode = SortMode::Unsorted;
    state.left.entries = names_of("e", 30).iter().map(|n| file_entry(n, 0)).collect();
    state.left.progress = crate::panel::ListingProgress::Streaming { count: 30 }; // still streaming
    state.left.cursor = 20;
    state.left.cursor_user_moved = true;
    state.left.scroll_offset = 10;

    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    assert!(state.left.reread_anchor.is_none(), "no anchor is captured from an incomplete listing");
    assert_eq!(state.left.cursor, 0, "the re-read resets exactly as it always has");
    assert_eq!(state.left.scroll_offset, 0);

    // ...and the fresh stream stays pinned to the top, as before.
    let (state, _) = listing_chunk(state, PanelSide::Left, vec![file_entry("a", 1), file_entry("b", 2)]);
    assert_eq!(state.left.cursor, 0);
    assert_eq!(state.left.scroll_offset, 0);
}

// ---------------------------------------------------------------------
// Panel-scrolling: Brief column-window and Tree reconciliation (task 2.3)
//
// These drive the reducer end to end, proving `reconcile_panel_viewport`'s
// Brief/Tree branches (task 2.1/2.2) are actually wired into
// `Command::MoveCursor` and `Command::TreeNodeExpanded`, on top of
// `panel::tests::brief_scroll_tests`/`tree_scroll_tests`'s pure coverage of
// the underlying clamp math.
// ---------------------------------------------------------------------

#[test]
fn brief_mode_interior_width_and_column_count_at_80x24_default_split() {
    // split 50/50 at width 80 -> left_w = 40, interior = 38 -> 3 columns
    // (byte-identical to `filecommand-tui::views::panel::render_brief_body`'s
    // `(inner_w / 12).max(1)`).
    let interior = panel_interior_width((80, 24), panel_split::DEFAULT_SPLIT_PERCENT, PanelSide::Left);
    assert_eq!(interior, 38);
    assert_eq!(brief_column_count(interior), 3);
    // Brief rows_h = the full body (no header row): panels_h = 22, no tab
    // strip at 1 tab, body_h = 20.
    assert_eq!(panel_viewport_rows((80, 24), DisplayMode::Brief, 1), 20);
}

#[test]
fn brief_mode_cursor_past_the_last_column_shifts_the_window_one_column_through_the_reducer() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Brief-mode: 20 rows_h, 3 columns (60-position window)
    state.left.display_mode = DisplayMode::Brief;
    state.left.sort_mode = SortMode::Unsorted;
    state.left.entries = (0..100).map(|i| file_entry(&format!("e{i}"), 0)).collect();
    state.left.cursor = 59; // column 2 (last of the window), row 19 -- the window's last position
    state.left.scroll_offset = 0;

    let (state, _) = update(state, Command::MoveCursor(CursorMove::Down(1)));

    assert_eq!(state.left.cursor, 60, "column 3, row 0: one step past the window's last column");
    assert_eq!(
        state.left.scroll_offset, 20,
        "shifts by exactly one column (20 positions), not further (additional-panel-modes \"Cursor past the last visible column shifts the window one column\")"
    );
    assert_eq!(
        state.left.scroll_offset % 20,
        0,
        "the offset stays on a rows_h-multiple column boundary (additional-panel-modes \"Window start stays on a column boundary\")"
    );
}

#[test]
fn brief_mode_quick_filter_re_clamps_the_column_window_through_the_reducer() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Brief-mode: 20 rows_h, 3 columns
    state.left.display_mode = DisplayMode::Brief;
    state.left.sort_mode = SortMode::Unsorted;
    state.left.entries = (0..100).map(|i| file_entry(&format!("e{i}"), 0)).collect();
    state.left.cursor = 65; // column 3
    state.left.scroll_offset = 60; // window starts at column 3, matching the cursor

    let (state, _) = update(state, Command::QuickFilterStart(PanelSide::Left));
    // Only "e6" and "e65"/"e6x" family remain a small handful of matches
    // near the front of the list, forcing the (now far too large) stale
    // offset to pull back.
    let (state, _) = update(state, Command::QuickFilterChar('e'));
    let (state, _) = update(state, Command::QuickFilterChar('6'));

    let visible = state.left.visible_indices();
    assert!(!visible.is_empty());
    let pos = visible.iter().position(|&i| i == state.left.cursor).unwrap();
    let start_col = state.left.scroll_offset / 20;
    let pos_col = pos / 20;
    assert!(
        start_col <= pos_col && pos_col < start_col + 3,
        "the cursor's column ({pos_col}) must be inside the re-clamped window starting at column {start_col} (panel-navigation \"Quick-filter narrowing re-clamps the offset\", extended to Brief's column space)"
    );
    assert_eq!(state.left.scroll_offset % 20, 0, "offset stays on a column boundary after re-clamping");
}

#[test]
fn tree_mode_cursor_below_the_bottom_scrolls_the_node_window_through_the_reducer() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 24); // Tree-mode body: 19 rows (body_h - 1 header)
    assert_eq!(panel_viewport_rows((80, 24), DisplayMode::Tree, 1), 19);
    state.left.display_mode = DisplayMode::Tree;
    let mut tree = TreeState::new(PathBuf::from(r"C:\"), DisplayMode::Full);
    let children: Vec<Entry> = (0..30).map(|i| dir_entry(&format!("d{i}"))).collect();
    tree.insert_children(&PathBuf::from(r"C:\"), children); // 31 nodes total (root + 30)
    tree.cursor = 18; // last visible row of a 19-row window starting at 0
    state.left.tree = Some(tree);

    let (state, _) = update(state, Command::MoveCursor(CursorMove::Down(1)));

    let tree = state.left.tree.as_ref().unwrap();
    assert_eq!(tree.cursor, 19);
    assert_eq!(
        tree.scroll_offset, 1,
        "shifts by exactly one row, mirroring Full mode (additional-panel-modes \"Tree cursor below the bottom scrolls the nodes\")"
    );
}

#[test]
fn tree_node_expanded_re_clamps_the_tree_offset_through_the_reducer() {
    let mut state = test_state(UiPhase::Panels);
    state.term_size = (80, 12); // a small Tree-mode body forces a small row count
    let rows = panel_viewport_rows((80, 12), DisplayMode::Tree, 1);
    state.left.display_mode = DisplayMode::Tree;
    let mut tree = TreeState::new(PathBuf::from(r"C:\"), DisplayMode::Full);
    let children: Vec<Entry> = (0..15).map(|i| dir_entry(&format!("d{i}"))).collect();
    tree.insert_children(&PathBuf::from(r"C:\"), children); // 16 nodes total (root + 15)
    tree.cursor = 12;
    tree.scroll_offset = 0; // stale at the current (small) row count: 12 is not in [0, rows)
    state.left.tree = Some(tree);

    // Expand a *different* node than the cursor's; it must not move the
    // cursor, but the node-list mutation still funnels through
    // reconciliation (additional-panel-modes "Tree mode scrolling").
    let more = vec![dir_entry("extra")];
    let (state, _) = update(state, Command::TreeNodeExpanded { panel: PanelSide::Left, path: PathBuf::from(r"C:\d0"), children: more });

    let tree = state.left.tree.as_ref().unwrap();
    assert_eq!(tree.cursor, 12, "expanding a different node doesn't move the cursor");
    assert_eq!(
        tree.scroll_offset,
        12 + 1 - rows,
        "the stale offset re-clamps to the minimal-shift window around the cursor once TreeNodeExpanded re-runs reconciliation"
    );
}

// ---------------------------------------------------------------------
// listing-request-guard: superseded listings are discarded and cancelled
// (panel-navigation "Superseded directory listings are discarded and
// cancelled"; panel-tabs "Switching or closing a tab mid-listing never
// mixes listings"; tasks 5.2–5.7)
// ---------------------------------------------------------------------

fn entry_names(state: &State, side: PanelSide) -> Vec<String> {
    state.panel(side).entries.iter().map(|e| e.name.to_string_lossy().into_owned()).collect()
}

/// A left panel showing a complete listing of `/left` with the one
/// directory `big` in it, so `Enter` starts a listing that the test then
/// leaves unfinished.
fn complete_left_with_big() -> State {
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.left.entries = vec![dir_entry("big")];
    state.left.progress = crate::panel::ListingProgress::Complete { count: 1 };
    state
}

#[test]
fn initial_state_tags_both_first_listings_with_distinct_request_ids() {
    let (state, effects) = State::initial(Theme::classic(), (80, 24), 0, 0, PathBuf::from("/l"), PathBuf::from("/r"), false);
    let left = state.left.listing_request.expect("the left panel's first listing is tagged");
    let right = state.right.listing_request.expect("the right panel's first listing is tagged");
    assert_ne!(left, right);
    assert!(effects.contains(&Effect::StartListing { panel: PanelSide::Left, path: PathBuf::from("/l"), request: left }));
    assert!(effects.contains(&Effect::StartListing { panel: PanelSide::Right, path: PathBuf::from("/r"), request: right }));
}

#[test]
fn backspace_out_of_a_slow_directory_mid_listing_drops_every_reply_of_the_old_listing() {
    // panel-navigation "Backspace out of a slow directory mid-listing" and
    // "Superseded completion does not complete the new listing".
    let (state, _) = update(complete_left_with_big(), Command::Enter);
    assert_eq!(state.left.cwd, PathBuf::from("/left/big"));
    let old = listing_request(&state, PanelSide::Left);

    // Backspace on the empty command line before `/left/big` finished.
    let (state, effects) = update(state, Command::ParentDir);
    assert_eq!(state.left.cwd, PathBuf::from("/left"));
    let new = listing_request(&state, PanelSide::Left);
    assert_ne!(old, new, "the parent listing is a new generation");
    assert!(effects.contains(&Effect::CancelRequest(old)), "the read of /left/big is signalled to stop: {effects:?}");
    assert!(effects.contains(&start_listing(&state, PanelSide::Left, "/left")));

    // The old worker's chunk — including its own `..` — arrives late.
    let stale = vec![Entry::parent_dir(), file_entry("in-big.txt", 1)];
    let (state, _) = update(state, Command::ListingChunk { panel: PanelSide::Left, entries: stale, request: old });
    assert!(state.left.entries.is_empty(), "no entry of /left/big, not even its `..`, reaches /left");
    assert_eq!(state.left.progress, crate::panel::ListingProgress::Streaming { count: 0 }, "the Reading… count covers only /left's entries");

    // Its completion must not complete the new listing either.
    let (state, _) = update(state, Command::ListingComplete { panel: PanelSide::Left, total: 2, unreadable: 0, request: old });
    assert!(state.left.progress.is_streaming(), "the mini-status keeps showing Reading… for the new listing");

    // The new listing's own replies are applied as usual.
    let (state, _) = listing_chunk(state, PanelSide::Left, vec![Entry::parent_dir(), dir_entry("big")]);
    let (state, _) = listing_complete(state, PanelSide::Left, 2);
    assert_eq!(entry_names(&state, PanelSide::Left), vec!["..", "big"]);
    assert_eq!(state.left.progress, crate::panel::ListingProgress::Complete { count: 2 });
}

#[test]
fn a_stale_listing_failure_sets_no_error_and_leaves_an_active_drag_alone() {
    // panel-navigation "Superseded failure does not mark the new listing
    // failed"; design D2: only an accepted failure clears a drag.
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![file_entry("a.txt", 1)];
    let (state, _) = update(state, Command::DragBegin { side: PanelSide::Left, name: OsString::from("a.txt"), op: JobKind::Copy });
    assert!(state.drag.is_some());

    // The right panel re-reads; the listing it supersedes then fails late.
    let old = listing_request(&state, PanelSide::Right);
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Right));
    let (state, _) =
        update(state, Command::ListingFailed { panel: PanelSide::Right, message: "Cannot read old: access denied".into(), request: old });
    assert_eq!(state.right.last_error, None, "no error is shown for the current directory");
    assert!(state.right.progress.is_streaming(), "the current listing continues normally");
    assert!(state.drag.is_some(), "a stale failure from a directory the user left does not cancel the drag");

    // The current listing's own failure still does both.
    let (state, _) = listing_failed(state, PanelSide::Right, "Cannot read right: access denied".into());
    assert_eq!(state.right.last_error.as_deref(), Some("Cannot read right: access denied"));
    assert!(state.drag.is_none(), "an accepted listing failure clears the drag (mouse-drag)");
}

#[test]
fn two_quick_rereads_list_each_entry_exactly_once() {
    // panel-navigation "Two quick re-reads do not duplicate entries".
    let mut state = test_state(UiPhase::Panels);
    state.left.progress = crate::panel::ListingProgress::Complete { count: 0 };
    let (state, effects) = update(state, Command::RereadPanel(PanelSide::Left));
    assert!(effects.iter().all(|e| !matches!(e, Effect::CancelRequest(_))), "a complete listing has no reader left to stop: {effects:?}");
    let first = listing_request(&state, PanelSide::Left);

    let (state, effects) = update(state, Command::RereadPanel(PanelSide::Left));
    let second = listing_request(&state, PanelSide::Left);
    assert_ne!(first, second);
    assert!(effects.contains(&Effect::CancelRequest(first)), "the first re-read, still streaming, is cancelled: {effects:?}");

    let entries: Vec<Entry> = (0..500).map(|i| file_entry(&format!("e{i:03}"), 0)).collect();
    // Both readers deliver everything before the first one noticed its flag.
    let (state, _) = update(state, Command::ListingChunk { panel: PanelSide::Left, entries: entries.clone(), request: first });
    let (state, _) = update(state, Command::ListingComplete { panel: PanelSide::Left, total: 500, unreadable: 0, request: first });
    assert!(state.left.entries.is_empty() && state.left.progress.is_streaming(), "the first generation's replies are dropped");
    let (state, _) = update(state, Command::ListingChunk { panel: PanelSide::Left, entries, request: second });
    let (state, _) = update(state, Command::ListingComplete { panel: PanelSide::Left, total: 500, unreadable: 0, request: second });

    let names = entry_names(&state, PanelSide::Left);
    assert_eq!(names.len(), 500);
    let distinct: std::collections::HashSet<&String> = names.iter().collect();
    assert_eq!(distinct.len(), 500, "each of the 500 entries is listed exactly once");
    assert_eq!(state.left.progress, crate::panel::ListingProgress::Complete { count: 500 });
}

#[test]
fn switching_tabs_mid_listing_cancels_it_and_the_interrupted_tab_rereads_on_return() {
    // panel-tabs "Switching tabs mid-listing" and "Returning to an
    // interrupted tab re-reads it".
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.left.entries = vec![dir_entry("big"), dir_entry("work")];
    state.left.progress = crate::panel::ListingProgress::Complete { count: 2 };
    // Tab 2 (opened with Ctrl+T) goes to /left/work and finishes listing it.
    let (state, _) = update(state, Command::OpenTab(PanelSide::Left));
    let mut state = state;
    state.left.cursor = 1; // "work"
    let (state, _) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, PathBuf::from("/left/work"));
    let (state, _) = listing_chunk(state, PanelSide::Left, vec![file_entry("w.txt", 1)]);
    let (state, _) = listing_complete(state, PanelSide::Left, 1);
    // Back on tab 1 (complete, untouched: no read), enter /left/big and
    // leave it streaming.
    let (state, effects) = update(state, Command::SwitchTab(1));
    assert!(effects.iter().all(|e| !matches!(e, Effect::StartListing { .. } | Effect::CancelRequest(_))), "tab 1 was complete: {effects:?}");
    let (state, _) = update(state, Command::Enter); // cursor 0: "big"
    assert_eq!(state.left.cwd, PathBuf::from("/left/big"));
    let old = listing_request(&state, PanelSide::Left);
    let (state, _) = listing_chunk(state, PanelSide::Left, vec![file_entry("b1.txt", 1)]);

    // Alt+2 while /left/big is still streaming.
    let (state, effects) = update(state, Command::SwitchTab(2));
    assert_eq!(state.left.cwd, PathBuf::from("/left/work"));
    assert!(effects.contains(&Effect::CancelRequest(old)), "the read of /left/big is stopped: {effects:?}");
    assert!(effects.iter().all(|e| !matches!(e, Effect::StartListing { .. })), "tab 2 was fully listed and is not re-read: {effects:?}");
    assert!(state.left.tabs[0].stale, "the interrupted tab is marked stale");
    assert_eq!(state.left.tabs[0].listing_request, Some(old), "the stashed tab keeps the id of the listing it was reading");

    // A late chunk of /left/big never reaches tab 2.
    let (state, _) = update(state, Command::ListingChunk { panel: PanelSide::Left, entries: vec![file_entry("b2.txt", 1)], request: old });
    assert_eq!(entry_names(&state, PanelSide::Left), vec!["w.txt"], "tab 2 shows only /left/work's entries");
    assert_eq!(state.left.progress, crate::panel::ListingProgress::Complete { count: 1 });

    // Alt+1: the interrupted tab starts a fresh read rather than showing a
    // partial listing that never completes.
    let history_before = state.dir_history.clone();
    let (state, effects) = update(state, Command::SwitchTab(1));
    assert_eq!(state.left.cwd, PathBuf::from("/left/big"));
    let fresh = listing_request(&state, PanelSide::Left);
    assert_ne!(fresh, old);
    assert!(effects.contains(&start_listing(&state, PanelSide::Left, "/left/big")), "a fresh read starts: {effects:?}");
    assert!(effects.iter().all(|e| !matches!(e, Effect::PersistHistory(_))), "the refresh is not a navigation: {effects:?}");
    assert_eq!(state.dir_history, history_before, "no fuzzy-jump visit is recorded");
    assert!(state.left.entries.is_empty(), "the partial listing is not shown");
    assert_eq!(state.left.progress, crate::panel::ListingProgress::Streaming { count: 0 }, "Reading… until the fresh read completes");
}

#[test]
fn an_interrupted_reread_still_keeps_the_users_place_when_its_tab_is_refreshed() {
    // panel-tabs "An interrupted re-read still keeps the user's place":
    // the anchor the interrupted re-read captured survives the stale-tab
    // refresh (design D6: `capture_anchor: false` for a streaming tab).
    let mut state = test_state(UiPhase::Panels);
    state.active = PanelSide::Left;
    state.term_size = (80, 24);
    state.left.sort_mode = SortMode::Unsorted;
    let mut names = names_of("e", 30);
    names[15] = "report.txt".to_string();
    let entries: Vec<Entry> = names.iter().map(|n| file_entry(n, 0)).collect();
    state.left.entries = entries.clone();
    state.left.progress = crate::panel::ListingProgress::Complete { count: 30 };
    state.left.cursor = 15;
    state.left.cursor_user_moved = true;
    state.left.scroll_offset = 5;
    // A second, complete tab to switch to, then back on tab 1.
    let (state, _) = update(state, Command::OpenTab(PanelSide::Left));
    let (state, _) = update(state, Command::SwitchTab(1));
    assert_eq!(state.left.cursor, 15);

    // Ctrl+R, then Alt+2 before the re-read finishes.
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    let old = listing_request(&state, PanelSide::Left);
    assert!(state.left.reread_anchor.is_some(), "the re-read captured the anchor");
    let (state, effects) = update(state, Command::SwitchTab(2));
    assert!(effects.contains(&Effect::CancelRequest(old)));
    assert!(
        effects.iter().all(|e| !matches!(e, Effect::StartListing { .. })),
        "switching to a fully listed, non-stale tab emits no StartListing: {effects:?}"
    );
    assert!(state.left.tabs[0].stale && state.left.tabs[0].reread_anchor.is_some(), "the stashed tab keeps its anchor");

    // Alt+1: the refresh keeps the anchor and the fresh listing restores it.
    let (state, effects) = update(state, Command::SwitchTab(1));
    assert!(effects.contains(&start_listing(&state, PanelSide::Left, "/left")));
    assert!(state.left.reread_anchor.is_some(), "the refresh does not clear the interrupted re-read's anchor");
    let (state, _) = listing_chunk(state, PanelSide::Left, entries);
    let (state, _) = listing_complete(state, PanelSide::Left, 30);
    assert_eq!(state.left.entries[state.left.cursor].name, OsString::from("report.txt"));
    assert_eq!(state.left.cursor, 15);
    assert_eq!(state.left.scroll_offset, 5, "the scroll offset from before Ctrl+R is restored");
}

#[test]
fn a_tab_interrupted_mid_navigation_refreshes_from_the_top() {
    // design D6: navigation cleared any anchor, so the fresh read of a
    // directory that never finished listing starts at the top.
    let mut state = complete_left_with_big();
    state.term_size = (80, 24);
    let (state, _) = update(state, Command::OpenTab(PanelSide::Left));
    let (state, _) = update(state, Command::SwitchTab(1));
    let (state, _) = update(state, Command::Enter); // /left/big, streaming
    let partial: Vec<Entry> = (0..10).map(|i| file_entry(&format!("b{i}"), 0)).collect();
    let (state, _) = listing_chunk(state, PanelSide::Left, partial);
    let mut state = state;
    state.left.cursor = 5;
    state.left.cursor_user_moved = true;
    assert!(state.left.reread_anchor.is_none());

    let (state, _) = update(state, Command::SwitchTab(2));
    let (state, effects) = update(state, Command::SwitchTab(1));
    assert!(effects.contains(&start_listing(&state, PanelSide::Left, "/left/big")));
    assert!(state.left.reread_anchor.is_none());
    assert!(state.left.entries.is_empty());
    assert_eq!((state.left.cursor, state.left.scroll_offset), (0, 0), "a never-completed directory starts from the top");
}

#[test]
fn closing_a_tab_mid_listing_cancels_it_and_the_neighbor_receives_none_of_it() {
    // panel-tabs "Closing a tab mid-listing".
    let (state, _) = update(complete_left_with_big(), Command::OpenTab(PanelSide::Left));
    let (state, _) = update(state, Command::Enter); // tab 2 → /left/big, streaming
    let old = listing_request(&state, PanelSide::Left);

    let (state, effects) = update(state, Command::CloseTab(PanelSide::Left));
    assert_eq!(state.left.tab_count(), 1);
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "the neighbor becomes active");
    assert!(effects.contains(&Effect::CancelRequest(old)), "the closed tab's read is stopped: {effects:?}");
    assert!(effects.iter().all(|e| !matches!(e, Effect::StartListing { .. })), "the complete neighbor is not re-read: {effects:?}");

    let (state, _) = update(state, Command::ListingChunk { panel: PanelSide::Left, entries: vec![file_entry("in-big.txt", 1)], request: old });
    let (state, _) = update(state, Command::ListingComplete { panel: PanelSide::Left, total: 1, unreadable: 0, request: old });
    assert_eq!(entry_names(&state, PanelSide::Left), vec!["big"], "the neighbor receives none of the closed tab's entries");
    assert_eq!(state.left.progress, crate::panel::ListingProgress::Complete { count: 1 });

    // Ctrl+W on the last remaining tab is a no-op and stops nothing.
    let (state, _) = update(state, Command::Enter); // /left/big again, streaming
    let (state, effects) = update(state, Command::CloseTab(PanelSide::Left));
    assert_eq!(state.left.tab_count(), 1);
    assert!(effects.is_empty(), "nothing happens when only one tab remains: {effects:?}");
}

#[test]
fn opening_a_tab_mid_listing_keeps_the_listing_flowing_into_the_new_tab() {
    // panel-tabs "New tab mid-listing keeps the listing".
    let (state, _) = update(complete_left_with_big(), Command::Enter); // /left/big, streaming
    let old = listing_request(&state, PanelSide::Left);
    let (state, _) = listing_chunk(state, PanelSide::Left, vec![file_entry("b1.txt", 1)]);

    let (state, effects) = update(state, Command::OpenTab(PanelSide::Left));
    assert!(effects.iter().all(|e| !matches!(e, Effect::CancelRequest(_))), "Ctrl+T stops nothing: {effects:?}");
    assert_eq!(state.left.tab_count(), 2);
    assert_eq!(listing_request(&state, PanelSide::Left), old, "the new tab inherits the in-progress listing");
    assert!(state.left.tabs[0].stale, "the original tab, now in the background, is stale");

    // Later chunks fill the new tab until it completes.
    let (state, _) = update(state, Command::ListingChunk { panel: PanelSide::Left, entries: vec![file_entry("b2.txt", 1)], request: old });
    let (state, _) = update(state, Command::ListingComplete { panel: PanelSide::Left, total: 2, unreadable: 0, request: old });
    assert_eq!(entry_names(&state, PanelSide::Left), vec!["b1.txt", "b2.txt"]);
    assert_eq!(state.left.progress, crate::panel::ListingProgress::Complete { count: 2 });

    // Activating the original tab re-reads /left/big instead of showing the
    // partial listing it held when Ctrl+T was pressed.
    let (state, effects) = update(state, Command::SwitchTab(1));
    assert_eq!(state.left.cwd, PathBuf::from("/left/big"));
    assert!(effects.contains(&start_listing(&state, PanelSide::Left, "/left/big")), "{effects:?}");
    assert!(state.left.entries.is_empty());
}

#[test]
fn swap_panels_is_still_refused_while_a_panel_streams_even_though_replies_carry_ids() {
    // Regression for the unchanged in-flight rule (design D7): the request
    // ids would make a mid-stream swap safe, but the refusal is kept by spec.
    let state = test_state(UiPhase::Panels); // both panels still streaming
    let before = state.clone();
    let (state, effects) = update(state, Command::SwapPanels);
    assert_eq!(state, before);
    assert!(effects.is_empty());
}

// ---------------------------------------------------------------------
// link-aware-listing: directory links navigate like directories, skipped
// entries are reported, and deleting a link asks once (panel-navigation
// "Directory links list as directories", "One unreadable entry does not
// fail a listing"; tasks 4.6–4.10)
// ---------------------------------------------------------------------

fn dir_link_entry(name: &str) -> Entry {
    Entry::new(OsString::from(name), EntryKind::Directory, 0, None, false, true)
}

fn dangling_link_entry(name: &str) -> Entry {
    Entry::new(OsString::from(name), EntryKind::File, 0, None, false, true)
}

#[test]
fn enter_descends_into_a_directory_link_and_parent_navigation_returns_to_the_links_directory() {
    let mut state = test_state(UiPhase::Panels);
    state.left.cwd = PathBuf::from("/proj/node_modules");
    state.left.entries = vec![dir_link_entry("lodash")];
    state.left.progress = crate::panel::ListingProgress::Complete { count: 1 };
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, PathBuf::from("/proj/node_modules/lodash"), "the link's own path becomes the panel's directory");
    assert!(effects.contains(&start_listing(&state, PanelSide::Left, "/proj/node_modules/lodash")), "{effects:?}");
    assert!(state.file_action_menu().is_none(), "Enter navigates rather than opening the file-action menu");

    // Ctrl+PgUp goes back to the directory containing the link, not to the
    // target's parent.
    let (state, effects) = update(state, Command::ParentDir);
    assert_eq!(state.left.cwd, PathBuf::from("/proj/node_modules"));
    assert!(effects.contains(&start_listing(&state, PanelSide::Left, "/proj/node_modules")), "{effects:?}");
}

#[test]
fn enter_on_a_dangling_link_does_not_start_a_listing() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dangling_link_entry("old")];
    state.left.progress = crate::panel::ListingProgress::Complete { count: 1 };
    let (state, effects) = update(state, Command::Enter);
    assert_eq!(state.left.cwd, PathBuf::from("/left"), "the panel's directory is unchanged");
    assert!(effects.iter().all(|e| !matches!(e, Effect::StartListing { .. })), "no listing starts: {effects:?}");
    assert!(state.file_action_menu().is_some(), "a dangling link is a file entry: Enter opens the file-action menu");
}

#[test]
fn listing_complete_with_skipped_entries_reports_the_count_until_the_next_listing() {
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = (0..49).map(|i| file_entry(&format!("e{i:02}"), 0)).collect();
    let request = listing_request(&state, PanelSide::Left);
    let (state, _) = update(state, Command::ListingComplete { panel: PanelSide::Left, total: 49, unreadable: 1, request });
    assert_eq!(state.left.last_error.as_deref(), Some("1 entry could not be read"));
    assert_eq!(state.left.progress, crate::panel::ListingProgress::Complete { count: 49 }, "the other 49 entries are listed");
    assert_eq!(state.left.entries.len(), 49);

    // The count is comma-grouped and pluralized.
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    assert_eq!(state.left.last_error, None, "the message clears the moment the panel lists again");
    let request = listing_request(&state, PanelSide::Left);
    let (state, _) = update(state, Command::ListingComplete { panel: PanelSide::Left, total: 0, unreadable: 1234, request });
    assert_eq!(state.left.last_error.as_deref(), Some("1,234 entries could not be read"));

    // A re-read that reads everything leaves no message.
    let (state, _) = update(state, Command::RereadPanel(PanelSide::Left));
    let (state, _) = listing_complete(state, PanelSide::Left, 0);
    assert_eq!(state.left.last_error, None);
}

#[test]
fn listing_failed_still_sets_the_inline_read_error_state() {
    let state = test_state(UiPhase::Panels);
    let (state, _) = listing_failed(state, PanelSide::Left, "Cannot read left: access denied".into());
    assert_eq!(state.left.last_error.as_deref(), Some("Cannot read left: access denied"));
    assert_eq!(state.left.progress, crate::panel::ListingProgress::Complete { count: 0 });
}

#[test]
fn deleting_a_directory_link_asks_once_while_a_real_directory_still_asks_twice() {
    // F8 on the junction: one confirmation, because Delete removes only the
    // link (design D5). The source still reports `is_dir`, as the job
    // engine's reparse-point handling expects.
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_link_entry("lodash"), dir_entry("real")];
    state.left.cursor = 0;
    let (state, _) = update(state, Command::RequestDelete);
    match &state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { sources, needs_second_confirm, .. }) => {
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].original_name, OsString::from("lodash"));
            assert!(sources[0].is_dir);
            assert!(!needs_second_confirm, "a directory link does not raise the non-empty-directory prompt");
        }
        other => panic!("expected the delete-confirm dialog, got {other:?}"),
    }

    // F8 on the real directory next to it still asks twice.
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_link_entry("lodash"), dir_entry("real")];
    state.left.cursor = 1;
    let (state, _) = update(state, Command::RequestDelete);
    match &state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { needs_second_confirm, .. }) => {
            assert!(needs_second_confirm, "a real directory requires the existing second confirmation");
        }
        other => panic!("expected the delete-confirm dialog, got {other:?}"),
    }

    // A selection mixing the link with a real directory asks twice for the
    // real directory's sake.
    let mut state = test_state(UiPhase::Panels);
    state.left.entries = vec![dir_link_entry("lodash"), dir_entry("real")];
    state.left.selected.insert(OsString::from("lodash"));
    state.left.selected.insert(OsString::from("real"));
    let (state, _) = update(state, Command::RequestDelete);
    match &state.phase {
        UiPhase::FileOpSetup(FileOpSetup::DeleteConfirm { sources, needs_second_confirm, .. }) => {
            assert_eq!(sources.len(), 2);
            assert!(needs_second_confirm);
        }
        other => panic!("expected the delete-confirm dialog, got {other:?}"),
    }
}
