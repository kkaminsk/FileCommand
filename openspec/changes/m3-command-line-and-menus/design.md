# Design — M3 Command line & menus

## Context

M3 builds the interactive command surface on top of the M1 event loop and M2 file operations (§9). It touches every layer of the two-crate workspace (§3): the `filecommand-core` `shell`, `panel`, `listing`, and `config` modules gain behavior, and `filecommand-tui` gains new `views/` renderers plus the terminal suspend/restore dance. The unifying constraints for this milestone are (1) the "instant NC feel" — command spawn latency and non-blocking drive/volume/count probing — and (2) the rendering policy of §4.11 (ANSI-16 named colors, CP437-heritage glyphs only, no spinners/animation).

Rendering stays a pure function of core state (§3.3): all the new state (command-line buffer, history cursor, active menu/pull-down selection, sort mode, drive list, Info values) lives in core and is mutated only through `core::update`, so it is snapshot-testable via `insta` + ratatui `TestBackend` (§8) with the injected `Clock` trait pinning the clock cell.

## Goals / Non-Goals

**Goals:**

- An always-live command line: printable keys route to it while a panel is focused, Enter runs the typed line through the configured shell with the TUI suspended, and the classic Up/Down history and Esc-clears-cursor semantics hold (§4.1).
- Instant command spawn on Windows via `cmd.exe /C` by default, with `shell =` in `config.toml` selecting PowerShell/pwsh and the documented ~200 ms+ latency tradeoff (§3.1, §6).
- F9 pull-down menu system (Left/Files/Commands/Options/Right) matching the §4.3 visuals and navigation exactly, overlaying the top row and hiding the clock (§4.1).
- Sort modes (Ctrl+F3..F7) with the header sort-arrow indicator and Ctrl+R re-read (§4.1, §5).
- Alt+F1/F2 drive select and Ctrl+L Info mode whose async values never block paint or input — drive letters and labels appear per §4.10's fill-in-place idiom.

**Non-Goals:**

- Maintaining an internal command-output buffer. Ctrl+O relies on the host terminal's native scrollback; FileCommand keeps no output history of its own (§5 note).
- Wiring every menu item to a working action. Items whose features land in later milestones (Find file, Fuzzy jump, Compare directories, Themes, tabs) render as menu entries but may be disabled or dispatch to existing M1/M2 commands; M3 owns the menu *framework* and the items that map to M3/earlier features.
- Tree/Quick View/Brief display modes (M5); only Info mode is added to the panel display-mode set here.
- Shell job streaming or output capture into the TUI — the shell runs on the real terminal with the TUI suspended, not inside a panel.

## Decisions

### D1: Shell passthrough via TUI suspend/restore, `cmd.exe /C` default

The `core::shell` module builds the command line (`shell + args + user text`) and the working directory (the active panel's path); the TUI binary performs the actual spawn because it owns the terminal (§3.2). Running a command: leave raw mode and the alternate screen, spawn the child inheriting stdio in the panel directory, wait, print "press any key", re-enter the alternate screen and raw mode, then re-read the panel (§7 refresh policy). Default shell is `cmd.exe /C` on Windows because spawning PowerShell costs 200 ms+ per command and kills the instant NC feel (§3.1); `config.toml` `shell =` selects PowerShell/pwsh with that tradeoff documented (§6). Enter on an executable/`.lnk`/PATHEXT target uses this same suspended-spawn path (§5). Keeping the command *construction* in core keeps it unit-testable (no terminal), while the unsafe terminal transition lives in the binary — the same split M2 used for fs jobs.

*Alternatives considered:* spawning inside a captured pipe and rendering output in a panel — rejected as non-authentic and scope-heavy; always using PowerShell — rejected on latency.

### D2: Command line owns printable-key routing; history keyed to non-empty buffer

While a panel is focused and no quick-search/dialog is active, printable keys append to the command-line buffer (§4.1). Up/Down navigate command history only while the buffer is non-empty; when empty they move the panel cursor. Esc clears the buffer, which is the explicit mechanism to hand Up/Down back to the panel (§4.1). This must interoperate with the §4.7 type-ahead jump (Alt+letter): while quick-search is active, plain printable keys extend the search and Esc/movement exit back to command-line typing — M3 must respect that mode flag so the two typing sinks never both consume a key. Command history persists to `history.json`, written atomically (§6), shared with the directory frecency store.

### D3: Ctrl+Enter/Ctrl+] paste; Windows key-delivery caveat

Ctrl+Enter pastes the cursor entry's filename and Ctrl+] pastes its full path onto the command line (§4.1, §5). Ctrl+Enter is reliable on Windows because crossterm reads native console input records directly, independent of ANSI/kitty protocol (§5 note); on other platforms it needs the kitty keyboard protocol and degrades to unavailable. Ctrl+] is ASCII 0x1D and works everywhere. These are documented behaviors, not spikes — the M1 key-delivery matrix (§9) already validates delivery; M3 just consumes the events.

### D4: Ctrl+O leaves the alternate screen, no internal buffer

Ctrl+O (panels on/off) leaves the alternate screen to expose the host terminal's scrollback containing prior command output; any key returns to the alternate screen and redraws (§5). FileCommand deliberately maintains no output buffer — output history is whatever the terminal retains. This reuses the same leave/enter-alternate-screen primitive as D1's shell spawn.

### D5: F9 menu as a modal overlay over core menu state

The F9 menu bar is a full-width black-on-cyan overlay replacing the top screen row and hiding the clock (§4.1, §4.3). Core holds the menu state (which of the five menus is active, which pull-down item is selected, open/closed). The pull-down is a single-line-framed box (black frame, black-on-cyan) hanging below its title; selected item white-on-black, disabled items grey (`menu.disabled`, white on cyan), separators drawn with `─` (§4.3, §4.11). Navigation: arrows/Enter/Esc, hotkey letters, and Left/Right moving between menus with the pull-down staying open; Esc closes the pull-down then the bar, restoring the top row and clock (§4.3). Rendered from the `menubar`/`menu.body`/`menu.highlight`/`menu.hotkey`/`menu.disabled` roles (§4.11), using only CP437 box glyphs (§4.11) so it snapshot-tests cleanly.

### D6: Sort modes as core `panel` state; re-read via `listing`

Sort mode (Name/Extension/Time/Size/Unsorted) is per-panel state in `core::panel`; Ctrl+F3..F6/Ctrl+F7 set it and re-sort the current entry list in place (§4.2 listing module, §5). The header column label for the sort column shows the `↓`/`↑` arrow indicator (e.g. `C:↓ Name`, §4.1), styled `panel.header` (§4.11). Ctrl+R re-reads the panel (§5, §7) — the same streaming read as M1 (§4.10), so a re-read of a large directory streams and shows `Reading… N` in the mini-status. Sort comparators are the property-tested comparators from §8; the sort itself is stable and operates on the already-gathered entry metadata (no re-`stat`, §3.1).

### D7: Non-blocking drive select and Info values on worker threads

Both `drive-select` and `info-panel` follow the §4.10 async idiom: never block paint or input. The drive-select dialog (Alt+F1/F2) enumerates drive letters synchronously via `GetLogicalDrives` (cheap) and shows them immediately; each volume label is fetched lazily on a worker thread and filled in when it resolves, so absent media (A:) and slow network drives never stall the dialog (§4.10, §5). Selecting an unavailable drive surfaces the panel error state rather than hanging (§5, §7). UNC paths are entered manually on the command line or in dialogs (§5). Info mode (Ctrl+L) renders stacked single-line-framed boxes; the version banner is the shared identity lines (§4.8, single source of truth), labels `info.label` cyan, values `info.value` bright-yellow (§4.11); async values (drive total/free, volume label, serial, file/dir counts) render as `…` until their background query resolves, then replace in place (§4.2, §4.10). All async values ride the existing worker-thread → event-queue → `core::update` plumbing (§3.3); results arriving for a stale drive/panel are discarded, mirroring the git-info staleness rule (§3.1). Target/panel/path equality alone isn't quite enough, though: two outstanding queries for the *same* target (a double Ctrl+R, or Alt+F1 cancelled and reopened before the first fetch lands) can complete out of order. `State` mints a monotonic request id (`State::next_request_id`) each time a query/fetch is (re-)issued, stamped onto `PanelState::info_request` / `DriveSelect::generation`; `InfoResolved`/`DriveLabelResolved` only apply when the incoming id still matches the outstanding one, so a superseded request's answer is dropped even when it targets the exact same panel or dialog.

### D8: Rendering & test policy

All new views emit ANSI-16 named colors only, using their §4.11 roles, and use only the CP437-heritage glyph set (`↓ ↑ ─ │ ┌ ┐ └ ┘ …` etc.) — no spinners or animation (§4.10, §4.11). Every new screen (command line with history, F9 menu bar + open pull-down, drive-select dialog with and without resolved labels, Info panel with `…` and resolved values, header sort arrow) gets an `insta` snapshot against ratatui `TestBackend` with pinned time (`Clock` trait), size, and locale (§8). Core-side logic (command construction, history navigation, sort comparators, menu state machine, drive enumeration parsing) is unit/property-tested in `filecommand-core` with the fs seam injected (§3.1, §8).

### D9: `cd` bypasses the shell-spawn path and navigates the panel directly

A typed `cd <path>` line is parsed (`core::update::parse_cd`/`resolve_cd_target`) and applied straight to the active panel's `PanelState` (§5), instead of going through D1's suspend/spawn/`RunShellCommand` path that every other command line uses. This is not a stylistic shortcut: `cd` only mutates the *child* process's working directory, and that child — mutated cwd and all — is gone the instant it exits, landing back in the parent FileCommand process exactly where it started. Shelling out to `cmd.exe /C cd ...` would therefore be an unconditional no-op, not merely a slower way to get the same result, so D1's spawn path is the wrong tool for this one command regardless of latency concerns. Treating `cd` as a first-class panel-navigation command instead reproduces NC's own behavior: relative segments, `..`, a bare drive letter (`D:`), and a manually typed UNC path all resolve the same way arrow/Enter-driven navigation would (§5), and the command still records to history (§6) like any other typed line. One consequence worth calling out: `cd` never spawns a subprocess, so — unlike every other command line — it never suspends/restores the terminal or shows the "press any key" prompt; there is no child output to show. See `cd_navigates_the_panel_instead_of_spawning_a_shell` in `core::update`'s tests for the regression coverage that keeps this intentional.

## Risks / Trade-offs

- **PowerShell shell latency (200 ms+/command) breaks the instant NC feel** -> Default to `cmd.exe /C` on Windows; make PowerShell/pwsh opt-in via `config.toml` and document the tradeoff (§3.1, §6).
- **Volume-label / free-space queries blocking on absent media or slow network drives** -> Enumerate letters synchronously but fetch labels and totals lazily on worker threads; render `…`/blank until resolved and never probe media in the render or input path (§4.10, §5). Selecting an unavailable drive yields the panel error state, not a hang (§7).
- **Ctrl+Enter unavailable on non-Windows terminals lacking the kitty protocol** -> Documented as best-effort off-Windows; Ctrl+] (ASCII 0x1D) always works as the path-paste path, and the binding is overridable in `config.toml` (§5).
- **Two competing printable-key sinks (command line vs §4.7 type-ahead jump)** -> A single mode flag in core arbitrates: quick-search mode consumes plain printables and Esc/movement exits back to command-line typing; only one sink ever sees a given key (§4.1, §4.7).
- **Menu items for not-yet-built features (Find file, Themes, tabs, Compare)** -> Render them as real entries but mark unavailable ones disabled (`menu.disabled`) or wire them to existing commands; M3 owns the framework, later milestones enable the items (Non-Goals).
- **TUI suspend/restore leaving the terminal in a bad state on a panicking child or error** -> Reuse the M-wide panic hook that restores raw mode and the alternate screen (§7), and make restore idempotent so both D1 (shell spawn) and D4 (Ctrl+O) recover cleanly.

## Open Questions

- Which Windows API surface for volume label and free/total space — `windows`/`windows-sys` bindings vs a thin FFI — given M3 adds no other heavyweight dependency? Resolve during M3 implementation; either satisfies D7's worker-thread requirement.
- Exact disabled/enabled item set per menu at M3 close, pending which later-milestone features are stubbed vs hidden.
