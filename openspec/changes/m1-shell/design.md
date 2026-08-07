# M1 — Shell — Design

## Context

FileCommand is a pre-implementation Rust project that recreates Norton Commander 5.5 as a keyboard-driven dual-panel terminal file manager (spec §1). M1 builds the walking skeleton every later milestone stands on: the workspace, the terminal-owning event loop, the pure state-update core (§3.3), the theme system (§4.11), two navigable Full-mode panels (§4.2), and the startup splash (§4.8). It deliberately stops short of file operations (M2), the command line/menus (M3), and the viewer/editor (M4/M5).

The design is constrained by three things the spec pins down and that this milestone must honor from the first commit: the `core` crate has **no terminal dependencies** so it stays unit-testable (§3.1); rendering is a **pure function of core state** with all mutation flowing through `core::update` (§3.3); and the visual output uses only ANSI-16 named colors plus CP437 box-drawing glyphs (§4.11). Three spec assumptions are also unverified and could force redesign of §4.1's F-key bar or the streaming-listing performance story, so M1 front-loads them as spikes (§9).

## Goals / Non-Goals

**Goals:**

- A compiling two-crate workspace (`filecommand-core` lib, `filecommand-tui` bin) with the dependency boundary enforced: core has zero ratatui/crossterm dependencies.
- A single-threaded UI event loop driving a pure `core::update(state, cmd) -> state`, with worker threads for directory reads (the only async work this milestone).
- Robust terminal ownership: alternate screen + raw mode acquired on start, always released on exit (normal, error, or panic).
- Two Full-mode panels rendering a real directory, cursor/Tab/Enter/parent navigation, F10 quit-with-confirm, and terminal reflow with the 80×24 floor and placeholder.
- A theme system that is the single styling authority for all rendering, shipping `nc-classic` and `nc-mono`.
- The startup splash with its timing, dismissal, disabling, resize, and below-minimum rules.
- Snapshot-tested rendering (ratatui `TestBackend` + `insta`) with a pinnable `Clock`.
- Three de-risking spikes producing written findings that gate/inform later work.

**Non-Goals:**

- File operations (copy/move/delete/mkdir), selection, and the `fs_ops` engine — M2.
- The command line executing shell commands, F9 menus, sort-mode switching UI, drive select, Info panel — M3. (M1 renders a static command-line prompt row and consumes Backspace-on-empty for parent nav, but does not spawn shells.)
- Viewer, editor, Brief/Info/Tree/Quick View modes, tabs, git info, quick filter, fuzzy jump — M4/M5.
- Full `config.toml` schema and user theme files on disk — M1 reads only `splash` and `theme`; the full loader is later.
- Actually shipping live F-key-bar relabeling — M1 only determines via spike (b) whether it is feasible.

## Decisions

### Crate split and the core/tui boundary

Two crates per §3: `filecommand-core` (platform-agnostic logic, no ratatui/crossterm) and `filecommand-tui` (owns terminal, event loop, rendering). Rationale: keeps all state logic unit-testable without a terminal, and forces the `core::update` discipline — the tui crate cannot smuggle rendering state into the model because it depends on core, not vice versa. `core` exposes `State`, `Command`, and `update`; it emits intents (e.g. "start listing this path") that the tui crate fulfills on worker threads and feeds back as commands. Alternative considered: a single crate with module boundaries. Rejected — nothing enforces the no-terminal-deps rule at compile time, and the spec explicitly mandates the split.

### Pure `core::update` and the data-flow model

Per §3.3: `key press → input map → Command → core::update(state, cmd) → new state`, with worker events (directory chunks, later fs progress) entering the same queue as commands. `core::update` is a pure, side-effect-free function returning the next state plus a list of effects to run; the tui event loop owns the effect execution (spawning threads, reading input). Rationale: rendering becomes a pure function of state (snapshot-testable), and unit tests drive `update` directly with synthetic command sequences — no terminal, no timing. Alternative considered: methods mutating `&mut State` with embedded I/O. Rejected — it entangles I/O with logic and defeats the snapshot/unit-test strategy of §8.

### Single-threaded UI, worker threads for reads

One thread owns the terminal and runs the event loop; directory listings run on worker threads and stream results back as chunks over a channel (§3.1 `listing` is async/streamed; §4.10 streaming listings). This milestone's only worker work is directory reads. Rationale: matches §3.2's "single-threaded UI with worker threads" target and the §2 requirement that first paint never waits on I/O — the splash paints as frame 1 while the first listing streams in behind it. The channel-drain step is part of each event-loop iteration and produces commands fed to `update`.

### Streaming listings and the mini-status counter

Listings render entries as chunks arrive, inserted in sorted position, cursor held on the first row until the user moves (§4.10). While a listing is incomplete the mini-status shows `Reading… N` with a running, comma-grouped count (§4.1); it reverts to the normal name/size/date/time line on completion. Rationale: directly implements the spec's period-authentic, animation-free loading idiom (§4.10) and is prerequisite to spike (c). Entry names are stored as `OsString`/`PathBuf` (Windows names are UTF-16, may contain unpaired surrogates) and displayed via lossy conversion; column widths use `unicode-width` display width so CJK/emoji names align (§3.1 `listing`). On Windows, metadata comes from the directory enumeration itself, not per-file `stat` calls (§3.1).

### Long-path abstraction from day one

Even though M1 only reads directories, all path handling in `listing` goes through the narrow internal fs seam and the `\\?\` long-path abstraction chosen in §3.1 (preferred over the machine-wide `LongPathsEnabled` opt-in because the prefix works unconditionally). Rationale: retrofitting long-path correctness after M2's fs_ops lands is error-prone; establishing the seam now also gives M2 its deterministic error-injection point. Alternative considered: raw `std::fs` now, abstraction later. Rejected — it bakes a `MAX_PATH` assumption into the first listing code.

### Terminal ownership and the panic hook

The tui crate acquires the alternate screen and raw mode on startup and guarantees restoration on every exit path via an RAII guard, plus a panic hook that leaves raw mode / alternate screen **before** the default hook prints the report (§3.2, §7 panic policy). Rationale: a panic in raw mode otherwise leaves the user's terminal unusable; the spec makes terminal restoration a hard requirement. The hook chains to the previously installed hook so the backtrace still surfaces.

### Resize, the 80×24 minimum, and the placeholder

The UI reflows on crossterm resize events. At or above 80×24 it lays out panels/command-line/F-key bar; below it, a `screen.placeholder` "terminal too small" message is drawn instead (§4, §4.11). Resize is handled uniformly for panels and splash: the splash box re-centers on resize, and shrinking below minimum mid-splash swaps to the placeholder and the splash does not return (§4.8). Rationale: one code path for "am I big enough" avoids divergent behavior between splash and panel states.

### Theme system: roles, color-depth policy, iconography

Themes are a `role → (ansi16, Option<truecolor>)` map (§3.1 `theme`, §4.11). ANSI-16 named colors are mandatory for every role; a `#RRGGBB` truecolor value is optional per role and used only when the terminal reports truecolor support, else the named color renders (§4.11 color-depth policy). Rendering emits standard 16-color attributes so the host terminal palette reproduces the DOS look. No 256-color indexed support. Iconography is ASCII plus the CP437 box-drawing/geometric glyph set only — **no Nerd Fonts, emoji, or file-type icons** (§4.11); differentiation is by color and case. Two built-in themes ship compiled in: `nc-classic` (default, the §4.11 role table) and `nc-mono` (white-on-black, inversions become black-on-white). Rationale: making the theme the single styling authority means every renderer looks up roles rather than hardcoding colors, which the snapshot tests then lock. Alternative considered: hardcoding `nc-classic` for M1 and adding themes later. Rejected — retrofitting a role indirection across every renderer is more churn than starting with it, and `nc-mono` is a cheap correctness check that no renderer hardcodes color.

### Startup splash as frame 1

The splash renders from static identity strings (name, version = crate version, copyright, tribute) as the very first frame, so it satisfies the <200 ms first-paint goal unconditionally while panels/listings initialize behind it (§2, §4.8). Minimum hold 800 ms via the injected `Clock`; any key dismisses immediately and that key event is **consumed** (never forwarded to command line or panels); disabled by `splash = false` config or `--nosplash` flag with the flag winning; box re-centers on resize; below-minimum shows the placeholder instead (§4.8). The identity lines are a single source of truth shared verbatim with the About dialog and Info banner (built later) — M1 defines them in one place in `core`. Rationale: the spec makes the splash the mechanism by which first paint is decoupled from all I/O.

### Injected `Clock` trait

The tui takes an injected time source (`Clock` trait) so both the on-screen clock and the splash's 800 ms hold are deterministic in tests (§8). Snapshot tests pin time, terminal size, and locale. Rationale: without an injected clock the splash-timing and clock-rendering scenarios are untestable; the spec's testing strategy names this seam explicitly.

### Snapshot testing with `TestBackend` + `insta`

Because rendering is pure over state, TUI screens are snapshot-tested with ratatui's `TestBackend` compared against committed `insta` snapshots (§8) — this is ratatui's documented recipe. M1 snapshots: Full panel (active/inactive), the splash, the "terminal too small" placeholder, the F-key bar, and the streaming `Reading… N` mini-status, each pinned to fixed time/size/locale. Rationale: locks the §4.11 visual contract cheaply and catches regressions from later milestones.

### Command line and shell posture (M1 scope)

M1 draws the command-line prompt row (`C:\PATH>_`, §4.1) and consumes Backspace-on-empty-command-line as parent-directory navigation (§5), but does **not** spawn shells — that is M3. The `shell` module's `cmd.exe /C` vs PowerShell decision (§3.1) is out of scope here; noting it only to mark the boundary. Rationale: parent-nav semantics depend on command-line emptiness, so the command-line buffer must exist in state now even though execution is deferred.

### De-risking spikes gate later milestones

Three spikes run as M1 tasks with written findings (§9): (a) a key-delivery matrix across Windows Terminal and conhost that documents an alternate binding for any undeliverable default (§5); (b) standalone modifier press/release detection — crossterm's Windows parser drops bare `VK_SHIFT`/`VK_CONTROL`/`VK_MENU` records (§4.1), so this spike verifies whether bare modifier events are obtainable another way (a crossterm release unlocking KKP on Windows, or a direct `ReadConsoleInput` bypass); its result **gates** whether §4.1's live F-key-bar relabeling ships or falls back to documenting variants in F1 Help; (c) a 100k-entry directory render benchmark validating the §2 responsiveness goal and the streaming design. Rationale: each de-risks an assumption that, if wrong, forces UI redesign — cheaper to learn now than in M5.

## Risks / Trade-offs

- **[Bare modifier events may be undeliverable on Windows]** -> Spike (b) determines feasibility before any relabeling code is written; the spec's documented fallback (drop live relabeling, document Ctrl/Alt variants in F1 Help) is the accepted degraded path, so M1 ships regardless of outcome.
- **[A default keybinding may not be delivered in some host terminal]** -> Spike (a)'s key-delivery matrix documents an alternate binding for each undeliverable default (§5); no default binding is assumed reliable until the matrix confirms it.
- **[100k-entry directories may render too slowly for the <200 ms / responsive goals]** -> Streaming listings decouple first paint from listing completion (splash is frame 1); spike (c) benchmarks the real render cost and, if it fails, the streaming/chunking or column-layout approach is revised while the surface area is still small.
- **[Non-Unicode Windows filenames could break rendering or later fs ops]** -> Names stored as `OsString`/`PathBuf` and rendered lossily with `unicode-width`; a core unit test covers non-Unicode `OsString` names (§8) from M1 so the invariant is locked before fs_ops depends on it.
- **[Establishing the theme-role indirection and long-path seam adds up-front cost for features M1 doesn't fully exercise]** -> Accepted deliberately: both are far cheaper to build in now than to retrofit across every renderer (theme) or every fs call site (long path) once M2–M5 code depends on them.
- **[A panic in raw mode leaves the terminal unusable]** -> RAII terminal guard plus a chained panic hook restores raw mode / alternate screen before the report prints (§7); exercised by an explicit test that panics inside the guarded scope.

## Open Questions

- Spike (b)'s outcome — whether bare modifier press/release is obtainable on Windows — is unknown until run; it decides whether §4.1 live relabeling is in scope for a later milestone or permanently a Help-documented fallback.
- Whether spike (c) surfaces a need to cap chunk size or debounce re-sorts during streaming; deferred to the benchmark's findings rather than pre-optimized.
