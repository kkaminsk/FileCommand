//! §8 integration smoke test: a scripted sequence of real `core::update`
//! commands, driven through the real `filecommand-tui` worker threads
//! against a real temp-directory fixture — no terminal is touched (raw
//! mode/`TerminalGuard` are TUI-shell concerns this test has no need of),
//! but every effect that does real I/O runs for real, so this exercises the
//! same reducer/worker/effect wiring `app::run`'s event loop does. Covers
//! F4 edit+save, Ctrl+T/Alt+1..9 tab switch, Ctrl+P filter, Ctrl+J jump, and
//! Alt+F7 find, asserting on both the resulting `core::State` and what
//! actually landed on disk.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use filecommand_core::editor::EditorState;
use filecommand_core::panel::DisplayMode;
use filecommand_core::{update, Command, Effect, PanelSide, State, UiPhase};
use filecommand_tui::worker;

/// How long to wait for a worker thread's reply before deciding the system
/// has quiesced. Generous relative to how fast these local-disk operations
/// actually complete, but short enough that a genuinely stuck thread still
/// fails the test promptly rather than hanging CI.
const QUIET_TIMEOUT: Duration = Duration::from_millis(150);

/// Execute the I/O-bearing effects this smoke test actually exercises,
/// feeding worker-thread results back over `tx`/`rx` the same way
/// `app::run_effects` does. Effects outside this test's scope (menu/drive-
/// select/viewer/...) are silently ignored — they simply don't arise from
/// the scripted command sequence below.
fn execute(effects: Vec<Effect>, tx: &Sender<Command>) {
    for effect in effects {
        match effect {
            Effect::StartListing { panel, path } => worker::spawn_listing(panel, path, tx.clone()),
            Effect::QueryGitInfo { panel, path, request } => worker::spawn_git_info_query(panel, path, request, tx.clone()),
            Effect::FindInSubtree { root, pattern, request } => worker::spawn_find_subtree(root, pattern, request, tx.clone()),
            // `Effect::OpenEditor`/`SaveEditor` are cheap enough to run
            // synchronously on the input path in the real app too (design
            // D1) — mirrored here rather than spawning a thread for them.
            Effect::OpenEditor { path } => {
                let reply = match EditorState::open(&path) {
                    Ok(filecommand_core::editor::LoadResult::Loaded(editor)) => Command::EditorOpened(Box::new(editor)),
                    Ok(filecommand_core::editor::LoadResult::TooLarge { size }) => Command::EditorTooLarge { path, size },
                    Err(e) => Command::EditorOpenFailed { message: e.to_string() },
                };
                let _ = tx.send(reply);
            }
            Effect::SaveEditor { editor, then_quit } => {
                let mut editor = *editor;
                let reply = match editor.save() {
                    Ok(()) => Command::EditorSaved { editor: Box::new(editor), then_quit },
                    Err(e) => Command::EditorSaveFailed { message: e.to_string() },
                };
                let _ = tx.send(reply);
            }
            _ => {}
        }
    }
}

/// Apply one command, execute its effects, then drain every worker reply
/// (and *their* resulting effects, transitively) until the system goes
/// quiet — so callers always see a settled `State` rather than one with
/// listings still streaming in.
fn step(mut state: State, cmd: Command, tx: &Sender<Command>, rx: &Receiver<Command>) -> State {
    let (s, effects) = update(state, cmd);
    state = s;
    execute(effects, tx);
    while let Ok(reply) = rx.recv_timeout(QUIET_TIMEOUT) {
        let (s, effects) = update(state, reply);
        state = s;
        execute(effects, tx);
    }
    state
}

fn write_file(path: &Path, contents: &str) {
    std::fs::write(path, contents).expect("write fixture file");
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("filecommand-tui-integration-smoke-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("left").join("sub")).unwrap();
        std::fs::create_dir_all(root.join("right")).unwrap();
        write_file(&root.join("left").join("report.txt"), "hello\n");
        write_file(&root.join("left").join("sub").join("nested.txt"), "inner\n");
        write_file(&root.join("right").join("other.txt"), "x\n");
        Fixture { root }
    }

    fn left(&self) -> PathBuf {
        self.root.join("left")
    }

    fn sub(&self) -> PathBuf {
        self.root.join("left").join("sub")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn scripted_smoke_sequence_covers_edit_save_tabs_filter_jump_and_find() {
    let fx = Fixture::new("main");
    let (tx, rx) = mpsc::channel::<Command>();

    let (state, effects) =
        State::initial(filecommand_core::theme::Theme::classic(), (80, 24), 1_000, fx.left(), fx.root.join("right"), false);
    execute(effects, &tx);
    let mut state = step(state, Command::Tick(1_000), &tx, &rx);

    // ---- Sanity: both panels loaded for real -------------------------
    assert!(state.left.entries.iter().any(|e| e.name == "report.txt"), "left panel must have listed report.txt: {:?}", state.left.entries);
    assert!(state.right.entries.iter().any(|e| e.name == "other.txt"));

    // ---- F4 edit + save ------------------------------------------------
    let report_idx = state.left.entries.iter().position(|e| e.name == "report.txt").expect("report.txt listed");
    state.left.cursor = report_idx;
    state = step(state, Command::RequestEditor, &tx, &rx);
    assert!(matches!(state.phase, UiPhase::Editor(_)), "F4 must open the built-in editor, got {:?}", state.phase);

    state = step(state, Command::EditorChar('X'), &tx, &rx);
    if let UiPhase::Editor(editor) = &state.phase {
        assert!(editor.is_modified(), "typing a character must mark the buffer modified");
    } else {
        panic!("expected the editor phase to still be open");
    }

    state = step(state, Command::EditorSave, &tx, &rx);
    let on_disk = std::fs::read_to_string(fx.left().join("report.txt")).expect("read saved file");
    assert!(on_disk.starts_with('X'), "the typed character must have been written to disk: {on_disk:?}");
    if let UiPhase::Editor(editor) = &state.phase {
        assert!(!editor.is_modified(), "F2 save must clear the modified flag");
    } else {
        panic!("expected the editor phase to still be open after a non-quitting save");
    }

    // F10 on an unmodified buffer exits directly (builtin-editor "Quitting
    // an unmodified buffer exits directly").
    state = step(state, Command::EditorRequestQuit, &tx, &rx);
    assert_eq!(state.phase, UiPhase::Panels, "F10 on an unmodified buffer returns straight to the panels");

    // ---- Ctrl+T / Alt+<n> panel tabs -----------------------------------
    assert_eq!(state.left.tab_count(), 1);
    state = step(state, Command::OpenTab, &tx, &rx);
    assert_eq!(state.left.tab_count(), 2, "Ctrl+T opens a second tab");
    // Navigate the (now active) second tab into `sub` — this also records
    // the frecency visit Ctrl+J will read from later.
    let sub_idx = state.left.entries.iter().position(|e| e.name == "sub").expect("sub listed");
    state.left.cursor = sub_idx;
    state = step(state, Command::Enter, &tx, &rx);
    assert_eq!(state.left.cwd, fx.sub(), "tab 2 navigated into sub");

    state = step(state, Command::SwitchTab(1), &tx, &rx);
    assert_eq!(state.left.cwd, fx.left(), "switching back to tab 1 restores its own directory");
    assert!(state.left.entries.iter().any(|e| e.name == "report.txt"));

    // ---- Ctrl+P quick filter --------------------------------------------
    state = step(state, Command::QuickFilterStart, &tx, &rx);
    state = step(state, Command::QuickFilterChar('r'), &tx, &rx);
    state = step(state, Command::QuickFilterChar('e'), &tx, &rx);
    let visible_names: Vec<String> =
        state.left.visible_indices().into_iter().map(|i| state.left.entries[i].name.to_string_lossy().into_owned()).collect();
    assert!(visible_names.contains(&"report.txt".to_string()), "{visible_names:?}");
    assert!(!visible_names.contains(&"sub".to_string()), "`sub` does not contain `re`: {visible_names:?}");
    state = step(state, Command::QuickFilterEnd, &tx, &rx);
    assert!(state.left.quick_filter.is_none(), "Esc clears the filter");

    // ---- Ctrl+J fuzzy jump ------------------------------------------------
    assert!(state.dir_history.iter().any(|e| e.path == fx.sub()), "the earlier tab-2 navigation into sub must be recorded");
    state = step(state, Command::FuzzyJumpOpen, &tx, &rx);
    assert!(state.fuzzy_jump.is_some());
    state = step(state, Command::FuzzyJumpConfirm, &tx, &rx);
    assert!(state.fuzzy_jump.is_none(), "the dialog closes");
    assert_eq!(state.left.cwd, fx.sub(), "Ctrl+J jumped the active panel to the most-frecent visited directory");

    // Back to `left`'s root before searching its subtree.
    state = step(state, Command::ParentDir, &tx, &rx);
    assert_eq!(state.left.cwd, fx.left());
    assert_eq!(state.active, PanelSide::Left);

    // ---- Alt+F7 find file ---------------------------------------------
    state = step(state, Command::FindFileOpen, &tx, &rx);
    assert_eq!(state.find_file.as_ref().unwrap().root, fx.left());
    for c in "nested".chars() {
        state = step(state, Command::FindFileChar(c), &tx, &rx);
    }
    state = step(state, Command::FindFileSubmit, &tx, &rx);
    {
        let dialog = state.find_file.as_ref().expect("find-file dialog still open");
        assert!(dialog.done, "the walk must have finished by the time `step` quiesces");
        assert_eq!(dialog.results.len(), 1, "exactly nested.txt should match `nested`: {:?}", dialog.results);
        assert_eq!(dialog.results[0].entry.name, "nested.txt");
    }
    state = step(state, Command::FindFileConfirm, &tx, &rx);
    assert!(state.find_file.is_none(), "the dialog closes on confirm");
    assert_eq!(state.left.cwd, fx.sub(), "find-file navigated the active panel to the match's containing directory");
    let cursor_name = state.left.entries.get(state.left.cursor).map(|e| e.name.to_string_lossy().into_owned());
    assert_eq!(cursor_name.as_deref(), Some("nested.txt"), "the cursor must settle on the matched entry");

    assert_eq!(state.left.display_mode, DisplayMode::Full, "no display-mode side effects leaked in along the way");
}
