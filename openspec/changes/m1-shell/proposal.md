# M1 — Shell

## Why

FileCommand has an approved design but no code. Before any file operations, viewer, or menus can be built, the project needs a working skeleton: a compiling Cargo workspace, a terminal that is owned and released safely, an event loop with a pure state-update core, a theme system, and two real panels the user can navigate. M1 delivers that walking skeleton as a demoable binary and, because three UI-critical assumptions (Windows key delivery, bare-modifier detection, 100k-entry render cost) could invalidate later milestones, it front-loads them as de-risking spikes.

## What Changes

- Establish a Cargo workspace with a `filecommand-core` library crate (no terminal dependencies, fully unit-testable) and a `filecommand-tui` binary crate on ratatui + crossterm.
- Implement the single-threaded UI event loop with worker threads and the pure `core::update(state, cmd) -> state` data-flow model that all state mutations pass through.
- Take ownership of the terminal: alternate screen + raw mode on entry, guaranteed restore on exit, and a panic hook that restores the terminal before printing the report.
- Handle terminal resize/reflow, enforce the 80×24 minimum, and draw a "terminal too small" placeholder below it.
- Render two side-by-side panels in Full mode (Name | Size | Date | Time) against a real directory, with double borders, centered path title, column headers, entry rows, and the mini-status line, including the streaming `Reading… N` count.
- Provide cursor movement, Tab to switch the active panel, Enter to descend into a directory, and parent-directory navigation (Ctrl+PgUp and Backspace on an empty command line).
- Implement F10 quit with a confirmation dialog.
- Implement the named-role theme system with the ANSI-16-mandatory / optional-truecolor color-depth policy, the CP437-only iconography rule, and the built-in `nc-classic` (default) and `nc-mono` themes.
- Implement the startup splash (frame-1 centered box on a blue backdrop, 800 ms minimum hold, immediate key dismissal with the key consumed, resize re-centering, below-minimum fallback) and the `splash = false` config / `--nosplash` flag disabling (flag wins).
- Run three de-risking spikes captured as tasks: (a) a key-delivery matrix in Windows Terminal and conhost, (b) standalone modifier press/release detection that gates §4.1 live F-key-bar relabeling, and (c) a 100k-entry directory render benchmark.

## Capabilities

### New Capabilities

- `application-shell`: Cargo workspace and crate split, the single-threaded UI event loop with worker threads, the pure `core::update` data-flow model, terminal ownership (alternate screen/raw mode), the panic hook that restores the terminal, and resize handling with the 80×24 minimum and "terminal too small" placeholder.
- `panel-navigation`: Panel state and Full-mode layout/rendering, cursor movement, Tab to switch active panel, Enter to descend, parent-directory navigation, and the streaming `Reading… N` mini-status.
- `theme-system`: The named role→color theme model, the ANSI-16-mandatory / optional-truecolor color-depth policy, the CP437-only iconography rule, and the built-in `nc-classic` and `nc-mono` themes.
- `startup-splash`: The frame-1 centered splash box on a blue backdrop, the 800 ms minimum hold, immediate key dismissal with the key consumed, `splash = false` / `--nosplash` disabling (flag wins), resize re-centering, and below-minimum fallback to the placeholder.

### Modified Capabilities

- None (greenfield project; no existing specs)

## Impact

- **New crate `filecommand-core`** — modules touched this milestone: `panel` (state, cursor, sort order), `listing` (streaming directory reads, `OsString`/`PathBuf` names, `unicode-width` column widths), `theme` (role model, `nc-classic`/`nc-mono`), `config` (minimal: `splash`, `theme`), plus the injected `Clock` trait and the narrow fs-access seam.
- **New crate `filecommand-tui`** — the terminal owner: event loop, panic hook, resize handling, `views/` renderers (panels, F-key bar, command line stub, splash, placeholder, quit-confirm dialog), `input/` keymap, and `--nosplash` CLI parsing.
- **Workspace dependencies introduced:** ratatui, crossterm, `unicode-width` (core + tui), and `insta` + ratatui `TestBackend` for snapshot tests. `git2`, `notify`, and fs-ops crates are deferred to later milestones.
- **No existing code affected** — this is the first implementation change on a pre-implementation repository.
