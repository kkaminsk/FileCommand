//! The F4 editors: the external editor spawn, the built-in editor's open and
//! its command handling, and the user-menu file edit.

use super::*;

// ---------------------------------------------------------------------
// F4 external editor (M4)
// ---------------------------------------------------------------------

/// F4 from a panel: resolve the cursor entry against the configured
/// `editor =` command and either dispatch the spawn effect or surface why
/// not (external-editor: F4 launches the editor, Config-driven external
/// editor command).
pub(super) fn handle_request_external_editor(state: &mut State) -> Vec<Effect> {
    let side = state.active;
    let panel = state.panel(side);
    let Some(entry) = panel.selected() else { return vec![] };
    let is_dir = entry.is_dir_like();
    let name = entry.name.clone();
    let cwd = panel.cwd.clone();
    match external_editor::resolve(state.editor.as_deref(), &cwd, &name, is_dir) {
        Ok(invocation) => {
            // Mirrors the `Command::ViewerOpened` clear: a prior failed F3/F4
            // attempt may have left `last_error` set, and successfully
            // dispatching the editor spawn has clearly moved past it — the
            // mini-status line must not keep showing the stale message while
            // the editor runs (§7 error policy). The eventual successful
            // return also clears it again via `Command::RereadPanel` ->
            // `begin_new_listing`, but that round trip only happens once the
            // editor process exits, so this clears it immediately rather
            // than leaving the stale message up for the whole edit session.
            state.panel_mut(side).last_error = None;
            vec![Effect::RunExternalEditor(invocation, side)]
        }
        Err(external_editor::TargetError::Unconfigured) => {
            state.panel_mut(side).last_error = Some(external_editor::NO_EDITOR_CONFIGURED_MESSAGE.to_string());
            vec![]
        }
        // The directory case is silently ignored, matching how F5/F6/F8
        // are no-ops with nothing eligible selected.
        Err(external_editor::TargetError::IsDirectory) => vec![],
    }
}

// ---------------------------------------------------------------------
// F4 built-in editor (M5)
// ---------------------------------------------------------------------

/// F4 from a panel: the external editor takes precedence when configured
/// (builtin-editor "External editor takes precedence") — reusing
/// `handle_request_external_editor`'s own entry/directory resolution rather
/// than duplicating it. Otherwise, on a file (not `..`, not a directory),
/// kick off the size-gated open; the size cap itself is enforced by
/// `EditorState::open` on the TUI side once `Effect::OpenEditor` runs
/// (builtin-editor "Editor invocation and size cap").
pub(super) fn handle_request_editor(state: &mut State) -> Vec<Effect> {
    if state.editor.is_some() {
        return handle_request_external_editor(state);
    }
    let side = state.active;
    let panel = state.panel(side);
    let Some(entry) = panel.selected() else { return vec![] };
    if entry.is_dir_like() {
        return vec![];
    }
    let path = panel.cwd.join(&entry.name);
    vec![request_open(state, side, PendingOpenKind::Editor, path)]
}

/// The editor body's visible row count for a given terminal height: the
/// header and F-key bar each reserve one row, exactly like the viewer's
/// `body_rows` (kept as a one-line duplicate here rather than a cross-crate
/// dependency, since `core::update` needs it for `ensure_caret_visible` and
/// cannot depend on `filecommand-tui`).
pub(super) fn editor_viewport(term_size: (u16, u16)) -> (usize, usize) {
    (term_size.1.saturating_sub(2).max(1) as usize, term_size.0.max(1) as usize)
}

/// Drive every command while the editor is open (gated in [`update`] by
/// `UiPhase::Editor`). Saving is the one operation here that needs I/O
/// (`EditorState::save` writes to disk), so it is dispatched as
/// `Effect::SaveEditor` and applied only once `Command::EditorSaved`/
/// `EditorSaveFailed` reply — mirroring how `handle_viewer` defers the
/// viewer's own I/O to effects and re-enters through their replies (design
/// D1/D2).
pub(super) fn handle_editor(state: &mut State, cmd: Command) -> Vec<Effect> {
    let mut effects = Vec::new();
    let UiPhase::Editor(mut editor) = std::mem::replace(&mut state.phase, UiPhase::Panels) else {
        unreachable!("handle_editor only called when phase is Editor");
    };
    let (rows, cols) = editor_viewport(state.term_size);
    // While a save is in flight on the worker thread, only its own reply
    // gets through: edits, caret moves and further save/quit commands are
    // ignored, so the buffer the worker is writing is exactly the buffer
    // the user sees marked clean when `EditorSaved` lands (application-
    // shell "Saving to a slow share shows progress and accepts no edits";
    // offload-blocking-io design D6).
    if editor.save_pending && !matches!(cmd, Command::EditorSaved { .. } | Command::EditorSaveFailed { .. }) {
        state.phase = UiPhase::Editor(editor);
        return effects;
    }
    match cmd {
        Command::EditorChar(c) => editor.type_char(c),
        Command::EditorNewline => editor.insert_newline(),
        Command::EditorBackspace => editor.backspace(),
        Command::EditorMove(m) => match m {
            EditorMove::Left => editor.move_left(),
            EditorMove::Right => editor.move_right(),
            EditorMove::Up => editor.move_up(),
            EditorMove::Down => editor.move_down(),
            EditorMove::Home => editor.move_home(),
            EditorMove::End => editor.move_end(),
            EditorMove::PageUp(rows) => editor.move_page_up(rows),
            EditorMove::PageDown(rows) => editor.move_page_down(rows),
        },
        Command::EditorToggleMode => editor.toggle_mode(),
        Command::EditorMark => editor.start_mark(),
        Command::EditorCut => editor.cut_selection(),
        Command::EditorCopy => editor.copy_selection(),
        Command::EditorPaste => editor.paste(),
        Command::EditorUndo => editor.undo(),

        Command::EditorSearchStart => editor.search_prompt = Some(String::new()),
        Command::EditorSearchChar(c) => {
            if let Some(p) = &mut editor.search_prompt {
                p.push(c);
            }
        }
        Command::EditorSearchBackspace => {
            if let Some(p) = &mut editor.search_prompt {
                p.pop();
            }
        }
        Command::EditorSearchCancel => editor.search_prompt = None,
        Command::EditorSearchConfirm => {
            if let Some(pattern) = editor.search_prompt.take() {
                if !pattern.is_empty() {
                    editor.find_next(&pattern);
                }
            }
        }

        Command::EditorReplaceStart => editor.replace_prompt = Some(ReplacePrompt::Pattern(String::new())),
        Command::EditorReplaceChar(c) => {
            if let Some(prompt) = &mut editor.replace_prompt {
                match prompt {
                    ReplacePrompt::Pattern(p) => p.push(c),
                    ReplacePrompt::Replacement { replacement, .. } => replacement.push(c),
                }
            }
        }
        Command::EditorReplaceBackspace => {
            if let Some(prompt) = &mut editor.replace_prompt {
                match prompt {
                    ReplacePrompt::Pattern(p) => {
                        p.pop();
                    }
                    ReplacePrompt::Replacement { replacement, .. } => {
                        replacement.pop();
                    }
                }
            }
        }
        Command::EditorReplaceCancel => editor.replace_prompt = None,
        // Enter: the pattern stage advances to the replacement stage (an
        // empty pattern instead cancels the prompt outright — nothing to
        // search for); the replacement stage performs the replacement
        // (builtin-editor "Replace substitutes a match").
        Command::EditorReplaceConfirm => {
            if let Some(prompt) = editor.replace_prompt.take() {
                match prompt {
                    ReplacePrompt::Pattern(pattern) if !pattern.is_empty() => {
                        editor.replace_prompt = Some(ReplacePrompt::Replacement { pattern, replacement: String::new() });
                    }
                    ReplacePrompt::Pattern(_) => {}
                    ReplacePrompt::Replacement { pattern, replacement } => {
                        editor.replace_first(&pattern, &replacement);
                    }
                }
            }
        }

        Command::EditorSave => {
            effects.push(Effect::SaveEditor { editor: Box::new(editor.clone()), then_quit: false });
            editor.save_pending = true;
        }
        Command::EditorSaved { editor: saved, then_quit } => {
            if then_quit {
                // Phase was already reset to `Panels` by the `mem::replace`
                // above; leaving it as-is is the exit.
                return effects;
            }
            editor = *saved;
            editor.save_pending = false;
            editor.quit_confirm = false;
        }
        Command::EditorSaveFailed { message } => {
            editor.save_pending = false;
            editor.save_error = Some(message);
            editor.quit_confirm = false;
        }

        Command::EditorRequestQuit => {
            if editor.is_modified() {
                editor.quit_confirm = true;
            } else {
                // Unmodified: exits directly (builtin-editor "Quitting an
                // unmodified buffer exits directly") — phase stays `Panels`.
                return effects;
            }
        }
        Command::EditorConfirmQuitSave => {
            effects.push(Effect::SaveEditor { editor: Box::new(editor.clone()), then_quit: true });
            editor.save_pending = true;
        }
        Command::EditorConfirmQuitDiscard => return effects,
        Command::EditorCancelQuit => editor.quit_confirm = false,

        _ => {}
    }
    // While the placeholder hides the editor, `term_size` is a one- or
    // two-row window; re-scrolling to it would wreck the viewport for a
    // reply (`EditorSaved`/`EditorSaveFailed`) the user never sees. The
    // `Resize` arm re-runs this once the terminal is back above the floor
    // (design D6).
    if !state.undersized {
        editor.ensure_caret_visible(rows, cols);
    }
    state.phase = UiPhase::Editor(editor);
    effects
}

// ---------------------------------------------------------------------
// Commands → Menu file edit (user-menu "Edit the user menu file from the
// Commands menu"; design D4)
// ---------------------------------------------------------------------

/// Open `usermenu.toml` (the path the app resolved at startup, kept in
/// `State::user_menu_path`) via the F4 editor path: the configured external
/// editor when `editor =` is set, else the built-in editor — the same branch
/// [`handle_request_editor`] takes, with the menu file supplied instead of
/// the cursor entry. The TUI reloads the menu when the editor returns and
/// feeds the result back as `Command::UserMenuReloaded`.
pub(super) fn handle_edit_user_menu_file(state: &mut State) -> Vec<Effect> {
    let path = state.user_menu_path.clone();
    if state.editor.is_some() {
        let side = state.active;
        let cwd = state.panel(side).cwd.clone();
        let file_arg = path.clone().into_os_string();
        return match external_editor::resolve(state.editor.as_deref(), &cwd, &file_arg, false) {
            Ok(invocation) => {
                // Mirrors `handle_request_external_editor`'s clear: a stale
                // mini-status must not outlive a successful dispatch.
                state.panel_mut(side).last_error = None;
                vec![Effect::RunExternalEditor(invocation, side)]
            }
            Err(external_editor::TargetError::Unconfigured) => {
                state.panel_mut(side).last_error = Some(external_editor::NO_EDITOR_CONFIGURED_MESSAGE.to_string());
                vec![]
            }
            // Unreachable (`is_dir` is `false`), kept explicit rather than
            // `unreachable!` so the wildcard stays a silent no-op.
            Err(external_editor::TargetError::IsDirectory) => vec![],
        };
    }
    let side = state.active;
    vec![request_open(state, side, PendingOpenKind::Editor, path)]
}
