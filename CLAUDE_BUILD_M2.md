# M2 — File Operations Build Task

You are building **Milestone 2 (File Operations)** of FileCommand, a keyboard-driven dual-panel terminal file manager in Rust that recreates Norton Commander 5.5.

## Project Context

The M1 Shell is already implemented and working. The codebase has:
- `filecommand-core` library crate (no terminal deps): modules `clock`, `config`, `identity`, `listing`, `panel`, `theme`, `update`
- `filecommand-tui` binary crate: `app`, `clock`, `input/`, `layout`, `style`, `terminal`, `worker`, `views/` (cmdline, keybar, panel, placeholder, quit_dialog, splash)
- All M1 tests pass (64 core + 12 tui + 9 snapshot + 6 snapshot + 1 panic restoration)
- The data flow: `key press → Command → core::update(state, cmd) → (state, Vec<Effect>)` with worker threads for directory reads

**Tech stack:** Rust, ratatui + crossterm, unicode-width. Windows-first.
**License:** MIT OR Apache-2.0 (dual).

## What You Must Build

Implement M2: selection and the four core file operations (Copy/F5, Rename-Move/F6, Make directory/F7, Delete/F8) with safe, cancellable, error-recoverable semantics. Add new modules to the existing codebase. Run `cargo build` and `cargo test` to verify everything compiles and passes.

### M2 Tasks (implement ALL of these)

#### 1. Module scaffolding and shared types (filecommand-core)
- Create `fs_ops` module in `filecommand-core` with submodules: `path` (\\?\ abstraction), `fs` (trait seam), `job` (job/event types), `conflict` and `error` (resolution state machines), `worker` (worker thread)
- Define `Job` type (kind: Copy/Move/Delete/Mkdir, source list keyed by original OsString, destination, resolved options) and worker→UI event enum (Progress, Conflict, Error, Done)
- Define request→reply channel types for pausing worker on conflict/error/cancel decisions, plus a shared cancel flag observed at file boundaries and between chunk copies
- Add core dialog-state enum consumed by TUI (destination input, overwrite conflict, progress, error recovery, delete confirm, skipped-files summary)

#### 2. Filesystem trait seam and long-path abstraction
- Define narrow internal fs trait: metadata/identity query (volume + file index), read-dir, create-dir, copy-file, rename, remove-file, remove-dir, set-attributes, reparse-point inspection
- Implement real Windows-backed fs implementation; ensure no fs_ops code calls std::fs outside the trait
- Implement fake fs for unit tests with deterministic injection of permission-denied, sharing-violation, disk-full at chosen operations
- Implement \\?\ path abstraction: full canonicalization before prefix, \\?\UNC\ for UNC paths; centralize so no caller hand-builds prefixed paths
- Route every fs-trait call through the path abstraction

#### 3. Selection state in panel (filecommand-core)
- Add per-panel selection set keyed by entry identity (original OsString), never by row index
- Implement Ins toggle-and-advance; parent-directory pseudo-entry (..) never selectable; no cursor wrap on last entry
- Implement wildcard group-select (+) and group-deselect (-) matching against original OsString name, additive/subtractive, excluding parent entry
- Implement invert-selection (*) over selectable entries, leaving parent unselected
- Compute `N files selected, X bytes` mini-status (directories contribute 0 bytes); revert to per-entry display when selection empty
- Preserve selection across cursor movement, re-sort, scroll; clear selection when panel changes directory

#### 4. Worker-thread job engine and progress
- Implement worker thread: accept Job, walk tree, perform operation, emit progress/conflict/error/done events over channel drained by event loop and folded through core::update
- Compute files_total/bytes_total with selected directories contributing 0 bytes while contained files count when job recurses; accumulate files_done/bytes_done
- Observe cancel signal at every file boundary and between chunk copies; emit terminal cancelled Done event

#### 5. Windows filesystem semantics in fs_ops
- F5 Copy: recursing into directories, preserving ADS, attributes, timestamps
- F6 Move: same-volume as instant rename; cross-volume as copy-then-delete (delete only after verified copy; leave source intact on failed copy)
- Identity-aware target-exists check (volume + file index, not name string) so case-only renames succeed
- Read-only attribute detection and clearing before overwrite and delete
- Reparse-point semantics: delete removes link not target contents; copy duplicates target content by default; recursion-cycle protection via visited file-identity set; never traverse junctions pointing inside source tree
- F7 Make directory

#### 6. Conflict and error resolution state machines
- Overwrite-conflict state machine (Overwrite/Skip/Rename/Overwrite All/Skip All) with "…All" latching auto-resolve policy; carry source and target size/date for display
- Per-file error-recovery state machine (Retry/Skip/Skip All/Abort) pausing job until user chooses
- Accumulate skipped items into list surfaced at end of job

#### 7. Panel read-error state and auto re-read
- Inline panel read-error state: listing failure enters error state (no panic/exit), offering re-read and drive-change actions; successful re-read replaces with normal listing
- Automatic re-read of affected panel(s) on job completion (including cancellation after partial changes); reconcile selection set against fresh listing (vanished entries drop out)

#### 8. TUI dialog views (filecommand-tui)
- Destination input dialog: primary black-on-cyan, double-line frame, bracket-and-dots input field, pre-filled with opposite panel path; Enter starts job, Esc aborts
- Overwrite-conflict dialog: source vs target size/date with timestamps through injected Clock; Overwrite/Skip/Rename/Overwrite All/Skip All
- Progress dialog: file counts, current file path, byte gauge with █ (dialog.gauge.filled, blue on cyan) and ░ (dialog.gauge.empty); Cancel control
- Error-recovery dialog: bright-white-on-red error style; Retry/Skip/Skip All/Abort
- Delete confirmation dialog: name single item or count for multi-selection; warn permanent deletion; second confirmation for non-empty directory
- End-of-job skipped-files summary dialog: list skipped items, shown only when 1+ items skipped

#### 9. Input routing and event-loop wiring
- Route Ins, +, -, * to core selection commands and F5/F6/F7/F8 to Copy/Move/Mkdir/Delete
- Spawn fs jobs onto worker thread; drain worker events back through event queue into core::update; keep UI non-blocking and Cancel responsive
- Wire conflict/error/cancel user choices from dialogs back onto worker reply channel; drive auto panel re-read on job completion

#### 10. Testing
- Core unit tests for selection semantics: Ins toggle/advance/no-wrap, parent non-selectable, +/- wildcard, * invert, mini-status counts/bytes with dirs at 0, persistence + clear-on-directory-change
- Core fs_ops unit tests against temp dirs and fake fs: multi-file copy/move/delete trees, same-volume rename vs cross-volume copy-then-delete, identity-aware case-only rename, read-only clear, reparse-point delete/copy/cycle-protection, cancellation mid-job, error injection (permission-denied/sharing-violation/disk-full)
- Core unit tests for conflict/error state machines: "…All" latching, Retry re-attempt, Abort, skipped-item accumulation
- Proptest for path joining including \\?\ and \\?\UNC\ prefixing; for overwrite-conflict-resolution state machine
- TUI insta + TestBackend snapshot tests for each dialog (destination input, overwrite conflict, progress with block gauge, error recovery, delete confirm, skipped-files summary), pinning time via injected Clock
- Integration test: navigate → select → copy → verify result on disk and panels re-read

## Theme roles for new dialogs (from §4.11)
- `dialog.primary`: black on cyan (body, frame, title)
- `dialog.error`: bright-white on red
- `dialog.input`: black on cyan (bracket-and-dots field)
- `button.normal`: black on white
- `button.focused`: black on bright-yellow
- `dialog.gauge.filled`: blue on cyan (█)
- `dialog.gauge.empty`: black on cyan (░)
- `panel.selected`: bright-yellow on blue

## Important Constraints

1. **filecommand-core MUST NOT depend on ratatui or crossterm**
2. **core::update is pure** — no I/O, no threads, no terminal side effects
3. **All rendering uses theme roles** — no hardcoded colors
4. **CP437 glyphs only** — no emoji, no Nerd Font, no file-type icons
5. **All fs access goes through the narrow trait seam** — no direct std::fs calls in fs_ops
6. **\\?\ long-path abstraction** — centralize prefixing, canonicalize before prefix
7. **Selection keyed by OsString identity** — never by row index
8. **Git workflow:** Work on the `build/m2-file-operations` branch. Commit when done.

## Success Criteria

- `cargo build` compiles both crates
- `cargo test` runs and passes all tests (existing M1 + new M2)
- The binary runs with working F5/F6/F7/F8 operations, selection with Ins/+/-/*

Start by reading the existing code to understand the M1 architecture, then build M2 on top of it. Create proper Rust module structures. Write comprehensive tests. Run cargo build and cargo test to verify. Commit your work when done.