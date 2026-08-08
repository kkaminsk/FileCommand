# M5 — Built-in editor & modern extras

## Why

M4 shipped the read-only viewer and the external-editor hook, so F4 is already useful, but FileCommand still lacks an in-process editor for quick edits and the small set of modern conveniences (§4.7) that distinguish it from a pure NC clone. M5 is the final v1 milestone: it completes the editing story with a minimal built-in editor and lands the modern extras plus the remaining panel display modes and the Help/About surface, yielding a feature-complete, demoable v1 binary.

## What Changes

- Add the F4 built-in minimal editor: insert/overwrite modes, line-based cut/copy/paste, non-regex search and search/replace, single-level undo, F2 save-in-place with CRLF/LF preservation, a modified indicator, a save-on-exit prompt, a <10 MB size cap (larger files fall back to the viewer with a notice), and the full editor chrome (§4.6).
- Add the Ctrl+P inline quick filter that narrows the panel to substring matches as the user types, cleared with Esc, and overridable in config (§4.7).
- Add Alt+letter type-ahead jump that moves the cursor to the first match, extends/shortens the pattern with printable keys/Backspace, and exits on Esc or movement (§4.7).
- Add Ctrl+J fuzzy directory jump backed by a frecency-ranked, session-persistent visited-directory index (§4.7).
- Add panel tabs (Ctrl+T / Ctrl+W / Alt+1..9) with per-tab directory+state and the compact tab strip (§4.1, §4.7).
- Add async git info: enclosing-repo detection on a worker thread, the branch-name border suffix, and the one-cell M/?/+ status-marker column, appearing together in a single reflow (§4.7, §4.10).
- Add the F2 user menu populated from `usermenu.toml` (§5, §6).
- Add the Alt+F7 find-file feature (§4.3, §5).
- Add the remaining panel display modes: Brief, Tree, and Quick View (§4.2).
- Add the F1 Help window and the secondary-style About dialog (§4.9).

## Capabilities

### New Capabilities

- `builtin-editor`: F4 in-process minimal editor — insert/overwrite, line-based cut/copy/paste, search and search/replace (no regex), single-level undo, F2 save-in-place with CRLF/LF preservation, modified indicator, save-on-exit prompt, <10 MB cap (larger files open in the viewer), and full editor chrome.
- `quick-filter`: Ctrl+P inline mini-status quick filter narrowing the panel to substring matches as typed, Esc to clear, overridable in config since it deviates from classic NC.
- `type-ahead-jump`: Alt+letter type-ahead quick-search moving the cursor to the first match, printable keys extending and Backspace shortening the pattern, Esc/movement exiting and returning printable keys to the command line.
- `fuzzy-jump`: Ctrl+J dialog listing fuzzy-matched, frecency-ranked previously visited directories, Enter navigating the active panel, with directory history persisted across sessions.
- `panel-tabs`: Ctrl+T new / Ctrl+W close / Alt+1..9 switch tabs with independent directory+state per tab and the compact tab strip shown only with 2+ tabs.
- `git-info`: Worker-thread git2 repo detection giving the branch-name border suffix and a one-cell M/?/+ status-marker column, pathspec-scoped to the panel directory, appearing in a single reflow and silently absent outside repos or on timeout.
- `user-menu`: F2 user menu populated from `usermenu.toml` label+command entries.
- `find-file`: Alt+F7 find-file feature searching the panel subtree for name-matching entries with jump-to-result.
- `additional-panel-modes`: Brief (three name-only columns), Tree (lazily-expanded directory tree that drives the opposite panel; Enter restores the prior list mode), and Quick View (viewer-style preview of the opposite panel's cursor file; SUB-DIR indicator for directories).
- `help-and-about`: F1 Help window (centered primary-style window, identity header, scrollable topic list starting on About FileCommand, topic pages, Help/Cancel buttons) and the secondary-style About dialog (identity lines, license, repository URL, OK button).

### Modified Capabilities

- None (greenfield project; no existing specs).

## Impact

- **`filecommand-core`**
  - `panel` — add Brief/Tree/QuickView display modes, per-panel tab list + active tab, and quick-filter substring narrowing to the panel state machine.
  - `quicksearch` — type-ahead jump state; fuzzy directory-jump index with frecency ranking.
  - `git_info` — repo detection, branch name, and pathspec-scoped per-file status markers via `git2`/libgit2 on a worker thread.
  - `listing` — expose lazy per-directory reads for Tree expansion and a find-file subtree walk.
  - `config` — `usermenu.toml` loader; quick-filter keybinding override; help/about identity strings (shared with splash/About).
- **`filecommand-tui`**
  - `views/` — editor renderer + chrome, tab strip, Brief/Tree/Quick View panel renderers, git branch suffix + marker column, Help window, About dialog, fuzzy-jump and find-file dialogs.
  - `input/` — editor keymap (insert/overwrite, Mark, search/replace, save, quit), type-ahead routing vs command line, tab and quick-filter bindings.
- **Dependencies** — `git2` (libgit2) enters the build for `git_info`; a fuzzy-match helper for `fuzzy-jump`/`find-file`. No new terminal dependencies.
- **Persistence** — `history.json` gains directory-frecency data; `usermenu.toml` is read.
- **Testing** — new `insta`/`TestBackend` snapshots (editor chrome, tab strip, Tree, Quick View, Help window, About dialog); core unit tests for editor buffer ops, quick-filter, type-ahead, fuzzy ranking, and `git_info` against fixture repos.
