# M1 — Shell — Tasks

## 1. Workspace and crate scaffolding

- [x] 1.1 Create the root Cargo workspace manifest declaring members `filecommand-core` and `filecommand-tui`, with dual `MIT OR Apache-2.0` license metadata and the MSRV policy noted.
- [x] 1.2 Scaffold the `filecommand-core` library crate with module stubs `panel`, `listing`, `theme`, `config`, `clock`, `identity`, and the pure `update` model module; add its dependencies (`unicode-width`) and confirm it declares NO `ratatui`/`crossterm` dependency.
- [x] 1.3 Scaffold the `filecommand-tui` binary crate depending on `filecommand-core`, `ratatui`, `crossterm`, and `unicode-width`; verify the dependency direction is one-way (tui → core, never core → tui).
- [x] 1.4 Add `insta` and ratatui `TestBackend` as dev-dependencies for snapshot tests, and `proptest` as a core dev-dependency.
- [x] 1.5 Confirm `cargo build` compiles both crates from the workspace root and `cargo test -p filecommand-core` runs without acquiring a terminal.

## 2. Core data-flow model (filecommand-core)

- [x] 2.1 Define the top-level `State` type (both panels, active-panel indicator, command-line buffer, splash/UI phase, active theme, size state) and the `Command` enum (key-derived commands plus worker-event commands).
- [x] 2.2 Define the `Effect` type used to request side effects (e.g. "start listing this path", "quit"), returned by `update` for the tui to execute.
- [x] 2.3 Implement `core::update(state, command) -> (state, Vec<Effect>)` as a pure, side-effect-free function (no terminal, filesystem, thread, or timing side effects).
- [x] 2.4 Ensure a directory-read-triggering command yields a state reflecting the request plus an effect intent to start the listing, without core spawning threads or reading the directory.
- [x] 2.5 Provide a conversion path so worker-produced events (e.g. listing chunks, completion) become `Command`s applied through the same `update` path as key-derived commands.

## 3. Theme system (filecommand-core)

- [x] 3.1 Define the fixed set of named roles required by every M1 renderer (`screen.backdrop`, `screen.placeholder`, `panel.frame`, `panel.title.active`, `panel.title.inactive`, `panel.header`, `panel.file`, `panel.directory`, `panel.cursor`, `panel.selected`, `panel.ministatus`, `keybar.number`, `keybar.label`, `commandline`, `clock`, `dialog.*` used by quit-confirm, and `splash.frame`/`splash.title`/`splash.version`/`splash.text`).
- [x] 3.2 Model a theme as a `role -> ColorSpec` map where each `ColorSpec` carries a mandatory ANSI-16 named foreground/background (from the 8 base + `bright-` variants, or the `none`/inherit sentinel) and an optional `#RRGGBB` truecolor value.
- [x] 3.3 Implement theme validation that rejects any theme missing a mandatory ANSI-16 value for a defined role, and a lookup API renderers call to resolve a role (guaranteeing no undefined role at render time).
- [x] 3.4 Implement the color-depth resolution: emit the `#RRGGBB` value only when the terminal reports truecolor support, otherwise the mandatory ANSI-16 named value; never emit a 256-color indexed attribute.
- [x] 3.5 Ship the compiled-in `nc-classic` theme matching the normative role table (e.g. `panel.directory` bright-white on blue, `panel.cursor` black on cyan, `keybar.label` black on cyan, `splash.title` bright-white on blue, `splash.frame` cyan on blue, `screen.placeholder` white on blue) and make it the default when no theme is configured.
- [x] 3.6 Ship the compiled-in `nc-mono` theme (white-on-black base; every `nc-classic` inversion becomes black-on-white; directories/selected stay bright-white; frames/`…`/gauges white) with no color carrying meaning not also carried by case/position/inversion.

## 4. Core config, clock, and identity (filecommand-core)

- [x] 4.1 Implement the minimal config reader exposing only `splash` (bool) and `theme` (name), tolerant of a missing file, defaulting `theme` to `nc-classic`.
- [x] 4.2 Define the injected `Clock` trait plus a real monotonic implementation (in tui) and a pinnable test implementation, so splash timing and the on-screen clock are deterministic.
- [x] 4.3 Define the identity lines (product name, `Version <crate-version>`, copyright, tribute) as a single source of truth in `core`, with version derived from the crate version; these are consumed verbatim by the splash.

## 5. Panel state and navigation logic (filecommand-core)

- [x] 5.1 Implement the `panel` state: current directory `PathBuf`, ordered entry list (names as `OsString`), cursor index, sort order, and listing-progress state (streaming vs complete with running count).
- [x] 5.2 Implement Name sort ordering with the `..` parent entry ordered first, and insertion of streamed entries in sorted position.
- [x] 5.3 Implement cursor movement commands (up, down, page, home, end) with clamping to a valid entry index and no directory change; hold the cursor on the first row while streaming until the user first moves it.
- [x] 5.4 Implement Tab active-panel toggle so exactly one panel is active and subsequent movement/navigation commands target the newly active panel.
- [x] 5.5 Implement Enter-to-descend: on a subdirectory (or `..`) set the panel's current directory to the target, request a new listing effect, and reset the cursor to the first entry.
- [x] 5.6 Implement parent-directory navigation triggered by Ctrl+PgUp and by Backspace-on-empty-command-line, with a no-op when the current directory is a filesystem root with no parent.
- [x] 5.7 Compute the mini-status content: `Reading… N` (comma-grouped count) while a listing is incomplete, reverting to the highlighted entry's name/size/date/time on completion.

## 6. Directory listing and long-path seam (filecommand-core)

- [x] 6.1 Define the narrow internal fs-access seam (trait) that `listing` reads through, giving later milestones a deterministic error-injection point.
- [x] 6.2 Route all path handling through the `\\?\` long-path abstraction from the first listing code.
- [x] 6.3 Implement streaming directory enumeration that emits entries as chunks (name + metadata sourced from the enumeration itself, not per-file `stat`), with a completion signal, designed to run on a worker thread.
- [x] 6.4 Compute display column widths using `unicode-width` display width and render names via lossy conversion of non-Unicode `OsString`, preserving alignment for CJK/emoji names.

## 7. Terminal ownership, event loop, and worker wiring (filecommand-tui)

- [x] 7.1 Implement an RAII terminal guard that enters the alternate screen and enables raw mode on startup and guarantees their release on every exit path (normal, error, panic).
- [x] 7.2 Install a panic hook that leaves raw mode and the alternate screen BEFORE the report prints and chains to the previously installed hook so the backtrace still surfaces.
- [x] 7.3 Implement the single-threaded event loop that drains one queue merging crossterm input events and worker events, converts each to a `Command`, applies `core::update`, executes returned effects, and redraws from the resulting state.
- [x] 7.4 Implement the worker-thread mechanism: fulfill "start listing" effects by spawning a worker that streams `listing` chunks/completion back over a channel into the event queue, keeping the UI thread responsive.
- [x] 7.5 Ensure the first painted frame never blocks on directory I/O (splash or panels paint while the initial listing streams behind them).

## 8. Resize, layout, and placeholder (filecommand-tui)

- [x] 8.1 Handle crossterm resize events and maintain terminal-size state, using a single size check (≥ 80×24) that governs both normal and splash states.
- [x] 8.2 Draw the `screen.placeholder` "terminal too small" message whenever below 80 columns or 24 rows, and reflow the normal layout when at or above the minimum (including recovery when resized back up).
- [x] 8.3 Implement the mid-splash shrink rule: replace the splash with the placeholder when it shrinks below minimum, and do not return the splash when the terminal grows back.

## 9. Rendering / views (filecommand-tui)

- [x] 9.1 Implement a theme-to-ratatui style adapter so every renderer resolves foreground/background by role lookup (no hardcoded colors, no theme-specific color branches) and emits only 16-color or truecolor attributes.
- [x] 9.2 Render the Full-mode panel: double-line border, centered path title (active = `panel.title.active` black-on-cyan, inactive = `panel.title.inactive` cyan-on-blue), `Name | Size | Date | Time` header with a `↓`/`↑` sort indicator on the active sort column.
- [x] 9.3 Render entry rows: directories in `panel.directory` style with `▶SUB-DIR◀` in the Size column, `▶UP--DIR◀` for the `..` entry, files in the file style, and the cursor row as a full-width inverse bar using `panel.cursor`; use only CP437 box-drawing/geometric glyphs and ASCII (no emoji/Nerd Font/file-type icons).
- [x] 9.4 Render the mini-status line inside the bottom border, showing `Reading… N` while streaming and the highlighted entry's name/size/date/time when complete.
- [x] 9.5 Render the static F-key bar (`keybar.number`/`keybar.label` roles) and the static command-line prompt row (`C:\PATH>_`) — display only, no shell execution.
- [x] 9.6 Render the two panels side-by-side against real streamed directory contents, wired to panel state.

## 10. Startup splash (filecommand-tui)

- [x] 10.1 Render the splash as frame 1 from the `core` identity lines: solid blue backdrop (`screen.backdrop`/`splash.*` roles), a horizontally/vertically centered double-line box containing name (`splash.title`), `Version <crate-version>` (`splash.version`), copyright and tribute (`splash.text`), with the terminal cursor hidden.
- [x] 10.2 Implement the 800 ms minimum hold measured via the injected `Clock`, after which the panels replace the splash even if the initial listing is still streaming.
- [x] 10.3 Implement immediate key dismissal before the minimum hold, consuming the dismissing key event so it is never forwarded to the command line or panels.
- [x] 10.4 Implement disabling: skip the splash (panels become frame 1) when `splash = false` in config or `--nosplash` is passed, with the flag winning over config.
- [x] 10.5 Implement splash re-centering on resize and the below-minimum-at-startup fallback to the placeholder (shrink-mid-splash behavior handled in 8.3).

## 11. Quit and input wiring (filecommand-tui)

- [x] 11.1 Implement F10 quit that raises a confirmation dialog rendered from `dialog.*` roles.
- [x] 11.2 Wire dialog confirmation to a clean exit via the RAII guard (raw mode disabled, alternate screen left before process termination).
- [x] 11.3 Implement the input keymap that maps crossterm key events to core `Command`s (movement, Tab, Enter, Ctrl+PgUp, Backspace, F10) and the `--nosplash` CLI flag parsing.

## 12. Testing (per §8 strategy)

- [x] 12.1 Core unit tests for `core::update` determinism and side-effect freedom: equal state + equal command yields equal next state and equal effects; directory-read command returns an intent effect without performing I/O; worker events re-enter through `update`.
- [x] 12.2 Core unit tests for panel logic: sort order with `..` first, cursor clamping at both ends, Tab focus toggle and command targeting, Enter descend (incl. `..`) with cursor reset, parent nav via Ctrl+PgUp and Backspace-on-empty, root-parent no-op.
- [x] 12.3 Core unit test for non-Unicode `OsString` filename handling (lossy display, width alignment) locking the invariant before fs_ops depends on it.
- [x] 12.4 Core unit tests for theme validation (reject missing ANSI-16 value), role completeness for every M1 role, color-depth resolution (truecolor when supported, ANSI-16 fallback, never 256-color), and `nc-classic`/`nc-mono` role tables.
- [x] 12.5 Core unit test for the mini-status counter formatting (`Reading… 12,345`) and revert-on-completion.
- [x] 12.6 Proptest for the sort comparator, and for path joining including `\\?\` prefixing.
- [x] 12.7 Snapshot tests (ratatui `TestBackend` + `insta`, pinned time/size/locale): Full panel active and inactive, splash under `nc-classic` and `nc-mono`, the "terminal too small" placeholder, the F-key bar, and the streaming `Reading… N` mini-status.
- [x] 12.8 Terminal-restoration test: a panic inside the guarded scope leaves raw mode / alternate screen before the report and delegates to the previous hook.

## 13. De-risking spikes (written findings, §9)

- [x] 13.1 Spike (a): run a key-delivery matrix across Windows Terminal and conhost, documenting an alternate binding for any undeliverable default binding (§5).
- [x] 13.2 Spike (b): standalone modifier press/release detection — determine whether bare `VK_SHIFT`/`VK_CONTROL`/`VK_MENU` events are obtainable (crossterm KKP release or a direct `ReadConsoleInput` bypass); record findings that gate whether §4.1 live F-key-bar relabeling ships or falls back to F1-Help-documented variants.
- [x] 13.3 Spike (c): 100k-entry directory render benchmark validating the responsiveness goal and the streaming design, recording whether chunk-size caps or re-sort debouncing are needed.
- [x] 13.4 Capture all three spikes' findings as written notes in the repo so they inform/gate later milestones.
