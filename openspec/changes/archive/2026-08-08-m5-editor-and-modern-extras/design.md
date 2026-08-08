# Design — M5 — Built-in editor & modern extras

## Context

M5 is the final v1 milestone (§9). By this point the architecture from §3 is in place: the two-crate Cargo workspace (`filecommand-core` with no terminal deps; `filecommand-tui` on ratatui + crossterm), the single-threaded UI event loop with worker threads for fs jobs / directory reads / git status, the `core::update(state, cmd) -> state` reducer that all mutations flow through, and the rendering-as-a-pure-function-of-state discipline. M5 adds ten capabilities that touch nearly every module but introduce no new architectural pattern — each is an extension of an existing seam (`panel`, `quicksearch`, `git_info`, `listing`, `config`, and the `views/` + `input/` layers).

The cross-cutting job of this design is to keep these additions faithful to the §4.11 rendering policy (ANSI-16 named roles, CP437-heritage single-cell glyphs only, no icons/emoji), consistent with §4.10's "static text, never spinners" async idiom, and testable through the existing `insta`/`TestBackend` snapshot and core-unit-test strategy (§8).

## Goals / Non-Goals

**Goals:**

- Land a genuinely minimal built-in editor that meets the explicit §4.6 v1 floor and nothing more (single-level undo, line-based selection, no regex), so scope stays contained.
- Keep every new async surface (git info especially) non-blocking and indicator-free per §4.10 — worker threads emit events into the same queue the reducer already drains; the UI never waits on I/O.
- Preserve the classic NC look: the tab strip is hidden with one tab, git info is silent outside repos, and all new chrome uses only §4.11 roles and glyphs.
- Reuse existing seams rather than adding subsystems: type-ahead and fuzzy-jump live in `quicksearch`; Brief/Tree/QuickView are `panel` display modes; all rendering stays a pure function of core state.
- Make everything snapshot-testable with pinned `Clock`, terminal size, and locale (§8), and keep all editor buffer / filter / ranking logic in `filecommand-core` unit tests independent of the terminal.

**Non-Goals:**

- No regex, no multi-level undo, no column/block selection, no syntax highlighting, no large-file editing — files ≥10 MB open in the viewer (§4.6).
- No filesystem watching for git or panels; git status is recomputed on panel re-read/navigation, not via a watcher (§7 refresh policy).
- No full-screen NCD tree workflow — only the Tree *panel mode* (§2 non-goals, §4.2).
- No repo-wide git status — queries are pathspec-scoped to the panel directory; untracked-directory contents are not enumerated (§3.1 `git_info`).
- No new persistence format — directory frecency rides in the existing `history.json`, written atomically (§6).

## Decisions

### D1 — Built-in editor: in-memory `Vec<Line>` gated at 10 MB, undo as a single snapshot batch

The editor loads the whole file into an in-memory line buffer in `filecommand-core` (no mmap — unlike the viewer, which streams multi-GB files per §4.5). The <10 MB cap (§4.6) makes full in-memory editing cheap and bounds worst-case memory; on open, a file ≥10 MB is redirected to the F3 viewer with a notice rather than loaded. Undo is the §4.6 "single-level undo batch": the editor keeps exactly one prior-state snapshot and F-undo swaps to it, which is trivially correct and matches the declared floor. Line-based selection means the selection model is a `[anchor_line, cursor_line]` range, not a character span — cut/copy/paste operate on whole lines, sidestepping grapheme-boundary complexity in the clipboard.

- *Rationale:* honors the explicit scope-containment mandate of §4.6; keeps all buffer logic terminal-free and unit-testable (§8).
- *Alternatives:* a rope/gap-buffer (rejected — over-engineered for <10 MB and a minimal floor); persistent undo history (rejected — §4.6 specifies single-level).

### D2 — CRLF/LF preservation and UTF-8 on save

On load the editor detects the dominant line ending and records it per-file; F2 writes back using the original terminator so a CRLF file stays CRLF and an LF file stays LF (§4.6). Text is decoded UTF-8 with lossy fallback for display consistent with the viewer, but the on-disk byte-accurate handling of endings is preserved so edits don't silently rewrite every line. Save is in-place (F2, "save-in-place"); the modified indicator (` *` after the header path, §4.6) is derived state from `buffer != saved_snapshot`, and F10 with unsaved changes raises the save-on-exit confirm dialog (§4.4 confirm style).

- *Rationale:* §4.6 calls out CRLF/LF preservation and UTF-8 explicitly; deriving the modified flag from state keeps it consistent with the pure-render rule.
- *Alternatives:* normalizing all endings to the platform default (rejected — corrupts mixed/foreign files and violates the preservation requirement).

### D3 — git info on a dedicated worker thread via `git2`, results merged in a single reflow

`git_info` (§3.1) runs libgit2 status on a dedicated worker thread because libgit2 status calls are blocking and not cancellable mid-call. The branch-name border suffix and the M/?/+ marker column are both absent until the query resolves, then appear together in one reflow (§4.10) — nothing is reserved or shown while pending, matching §4.7's "silent and absent." Status is pathspec-scoped to the panel directory (not repo-wide) and untracked-directory contents are not enumerated (§3.1). "Timeout" means a stale result is discarded and the repo is marked "no info" for the session; the abandoned thread is left to finish since the call can't be interrupted. Results are keyed to the panel's current directory + a generation counter so a result that arrives after the user has navigated away is dropped.

- *Rationale:* directly implements §3.1/§4.7/§4.10; the generation counter is the standard guard against stale async results landing in the reducer.
- *Alternatives:* `gitoxide` or a `git status --porcelain` subprocess (noted as fallbacks in §3.1 if libgit2 proves too heavy — deferred, not chosen now); a shared worker thread (rejected — a slow status call would head-of-line-block other panels).

### D4 — Panel tabs live in `panel` state; the strip is a `views/` concern shown only with 2+ tabs

Per §3.1 the `panel` module already owns the "per-panel tab list and active tab." A tab is a full independent panel state (directory, cursor, selection, sort, filter, display mode) per §4.7. Ctrl+T/Ctrl+W/Alt+1..9 are reducer commands that push/pop/select within the active panel's tab list. Rendering the compact strip is purely a `views/` concern: hidden with one tab (the panel keeps its full height, preserving the classic look), and with 2+ tabs the panel shrinks by one row for the strip (§4.1). The strip's stepwise label shrinking (` n:NAME `→` n:NAM… `→` n `) and active-tab-visible scrolling with `◄`/`►` markers are layout math over §4.11 `tab.active`/`tab.inactive` roles and single-cell glyphs.

- *Rationale:* matches the §3.1 module ownership and keeps the reducer/render split clean; the strip logic is deterministic and snapshot-testable.
- *Alternatives:* modeling tabs in the TUI layer (rejected — would split panel state across crates and break core unit-testability).

### D5 — Type-ahead vs command line: an explicit panel input mode resolves the printable-key conflict

§4.1 sends printable keys typed over a focused panel to the command line; §4.7's type-ahead also consumes printable keys. The resolution (already stated in §4.7) is an explicit mode: Alt+letter enters type-ahead mode, after which *plain* printable keys extend the pattern (shown in the mini-status via `panel.ministatus` role) and Backspace shortens it; Esc or any movement key exits and returns printable keys to the command line. This is a small state flag on the panel that the `input/` router checks before dispatching printable keys, so the two consumers never race.

- *Rationale:* the mode flag is the minimal change that satisfies both §4.1 and §4.7 without a separate widget; it's a pure reducer state transition.
- *Alternatives:* a modal popup search box (rejected — non-authentic and heavier than the mini-status idiom NC uses).

### D6 — Quick filter (Ctrl+P) and fuzzy jump (Ctrl+J) reuse existing seams

Quick filter is a substring narrowing already anticipated by `listing` ("quick-filter substring") and `panel`; it renders inline in the mini-status (`panel.ministatus`), narrows to matches as typed, and clears on Esc. Because Ctrl+P deviates from classic NC's meaning, the binding is overridable in `config.toml` (§4.7) — it flows through the existing keymap-override mechanism, not a special case. Fuzzy jump (Ctrl+J) is backed by the `quicksearch` "fuzzy directory-jump index (frecency-ranked list of visited directories, persisted)"; the ranking (recency × frequency) and match logic live in `filecommand-core` and are unit-tested independent of the dialog. Directory history persists in `history.json` written atomically (§6), shared with command history.

- *Rationale:* both are extensions of named §3.1 responsibilities; keeping ranking/matching in core keeps it deterministic and testable (§8).
- *Alternatives:* a new persistence file for frecency (rejected — §6 already assigns it to `history.json`).

### D7 — Tree and Quick View drive the *opposite* panel; both reuse existing renderers/reads

Tree mode uses `listing`'s lazy per-directory reads (directories read on expand, not a full-drive scan) per §4.2; moving the cursor updates the *opposite* panel to the highlighted directory, and Enter returns *this* panel to its prior list mode at the chosen directory. Quick View renders the opposite panel's cursor file exactly like the viewer's text mode (wrap on, lossy UTF-8, `viewer.text` role) with a centered `▶SUB-DIR◀` for directories (§4.2) — it reuses the M4 viewer's text-head rendering rather than a new path. Brief is three name-only columns using `unicode-width` for alignment (§3.1). All three are `PanelDisplayMode` variants the reducer already enumerates (§3.1 lists Brief/Full/Info/Tree/QuickView).

- *Rationale:* reuses `listing` lazy reads and the M4 viewer renderer; the opposite-panel coupling is a reducer effect, keeping render pure.
- *Alternatives:* eager full-tree scan (rejected — §4.2 mandates lazy expand and §2 forbids up-front drive scanning cost).

### D8 — User menu, find-file, and Help/About are static-data + reducer surfaces

The F2 user menu is parsed from `usermenu.toml` (label+command entries, §6) by `config`; selecting an entry runs the command through the existing `shell` passthrough (`cmd.exe /C` by default, PowerShell opt-in per §3.1) in the panel's directory. Alt+F7 find-file walks the panel subtree via `listing` for name matches and jumps the cursor to a chosen result. The F1 Help window and About dialog render from *static text compiled into the binary* (§4.9): the identity lines (name, version, copyright, tribute) are a single source of truth shared verbatim with the splash (§4.8) and the Info-panel banner (§4.2). Help is the §4.4 primary dialog style (black on cyan, double-line frame); About is the secondary grey style (black on white, single-line frame) with `License: MIT OR Apache-2.0` and the repository URL (§4.9, §10).

- *Rationale:* keeps the identity strings DRY across three surfaces and keeps Help content build-time static (no I/O, snapshot-stable).
- *Alternatives:* loading help text from external files (rejected — §4.9 specifies compiled-in static text).

### D9 — Windows path & shell fidelity carries through unchanged

All new file access (editor load/save, find-file walk, Tree/Quick View reads) goes through the same narrow internal fs trait and the `\\?\` long-path abstraction (§3.1) chosen over the registry/manifest opt-in, so long paths and error injection keep working. Filenames remain `OsString`/`PathBuf` end-to-end (find-file matching and tab labels use lossy display conversion with the visual marker only for rendering, never for fs ops), preserving the non-Unicode handling guarantee (§3.1, §8).

- *Rationale:* consistency with the established fs seam; avoids reintroducing path bugs the abstraction already solves.
- *Alternatives:* direct `std::path` calls in new code (rejected — bypasses the long-path abstraction and the test seam).

## Risks / Trade-offs

- **[libgit2 adds a C build dependency and can be slow on large/networked repos]** -> Isolate on a dedicated worker thread (D3); guard results with a directory+generation key and discard stale ones; degrade to "no info" silently on timeout (§3.1); `gitoxide`/`git status --porcelain` remain documented fallbacks if it proves too heavy.
- **[The minimal editor may feel underpowered (no regex, single undo, line-only selection)]** -> This is a deliberate §4.6 scope floor, not an accident; the `config.toml` external-editor hook (shipped M4) covers heavier editing, and F4 already worked via that hook before the built-in landed.
- **[Full in-memory editing risks memory blowups on huge files]** -> Hard 10 MB cap redirects larger files to the streaming viewer with a notice (§4.6, D1); the cap is checked before load.
- **[Ctrl+P / type-ahead can collide with command-line typing]** -> Explicit panel input mode (D5) plus an overridable Ctrl+P binding (D6, §4.7) keep the printable-key routing unambiguous; both flow through the existing keymap-override mechanism.
- **[CRLF/LF or non-Unicode names could be corrupted on save]** -> Per-file line-ending detection and byte-accurate write-back (D2); all fs ops use the original `OsString` through the fs trait (D9); covered by core unit tests including non-Unicode filenames (§8).
- **[Tab strip overflow and stepwise shrinking are fiddly layout math]** -> Deterministic pure-function layout over §4.11 roles/glyphs, locked down with `insta`/`TestBackend` snapshots at pinned sizes (§8, D4).
- **[Stale async results (git, drive labels) landing after navigation]** -> Generation-counter guard on every worker result so late arrivals are dropped (D3), matching §4.10's single-reflow appearance rule.

## Open Questions

- Fuzzy-match implementation for `fuzzy-jump`/`find-file`: hand-rolled subsequence scorer vs a small crate — decide during implementation, keeping the scorer in `filecommand-core` for testability either way.
- Whether find-file result navigation should open the target in a new tab (D4) or navigate the active tab in place — default to in-place, revisit if it proves disruptive.
