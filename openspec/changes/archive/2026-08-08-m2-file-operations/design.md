# Design: m2-file-operations

## Context

M2 implements the Norton Commander core file operations — selection and F5/F6/F7/F8 — on top of the M1 shell. The spec fixes the surrounding architecture: a two-crate workspace (§3) with a platform-agnostic `filecommand-core` (no ratatui/crossterm) and a `filecommand-tui` binary that owns the terminal, event loop, and rendering. `fs_ops` (§3.1) is the engine at the heart of this milestone: copy/move/delete/mkdir as cancellable jobs on a worker thread that emit progress and error events, blocking on Retry/Skip/Abort/SkipAll decisions and resolving overwrite conflicts. The data flow (§3.3) is `key press → Command → core::update → new state`, with fs jobs spawned onto a worker thread whose progress/error/done events return through the event queue into `core::update`. Rendering is a pure function of core state (§3.3), and the rendering policy (§4.11) fixes dialog styling, the `█`/`░` block gauge, and the ANSI-16 palette. This document captures the cross-cutting decisions that span the four capabilities of this change; per-requirement behavior lives in the delta specs.

## Goals / Non-Goals

**Goals:**

- Implement selection, Copy, Rename/Move, Make directory, and Delete with the exact dialogs, confirmations, and recovery flows of §4.4 and §7.
- Keep the UI single-threaded and responsive: long operations run on a worker thread and never block input or paint (§3.2, §3.3); the progress dialog and Cancel remain live throughout.
- Make all fs behavior deterministically testable in `filecommand-core` with no terminal and no real disk faults, via the narrow internal fs trait seam (§3.1, §8).
- Get Windows semantics right: `\\?\` long paths, same-volume rename vs cross-volume copy-then-delete, identity-aware case-only rename, ADS/attribute/timestamp preservation, and reparse-point handling with cycle protection (§3.1, §7).
- Auto re-read affected panels on completion (§7 refresh policy).

**Non-Goals:**

- No recycle bin — deletes are permanent in v1, and the confirmation says so (§7).
- No filesystem watching — refresh is automatic-after-own-ops plus manual Ctrl+R; `notify` is a v2 candidate (§7).
- No directory sizing — selected directories contribute 0 bytes to the mini-status and to progress totals (§4.1).
- No command line, menus, sort/drive dialogs, viewer, or editor (M3–M5). Attributes dialog (§4.3 Files menu) is out of scope for M2.
- No archive/FTP/virtual file systems (§2 non-goals).

## Decisions

### D1 — `fs_ops` engine on a worker thread; UI stays single-threaded

Per §3.2/§3.3 the UI thread never does blocking I/O. A file operation is packaged as a `Job` (kind + source list + destination + resolved options) and handed to a worker thread. The worker walks the tree, performs the operation, and emits events — `Progress { current_file, bytes_done, bytes_total, files_done, files_total }`, `Conflict{…}`, `Error{…}`, `Done{…}` — over a channel drained by the event loop and folded into state through `core::update`. Decisions that must pause the worker (conflict resolution, per-file error recovery, cancellation) travel back on a reply channel; the worker blocks awaiting the user's choice while the UI stays fully interactive. Rationale: matches the spec's data-flow diagram and keeps rendering a pure function of state. Alternative considered: async/`tokio` — rejected as heavier than needed for a single sequential job and inconsistent with the spec's "worker threads" (plural std threads) framing.

### D2 — Narrow internal fs trait seam

All file-system access inside `fs_ops` goes through a small internal trait (metadata/identity query, read-dir, create-dir, copy-file, rename, remove-file/dir, set-attributes, reparse-point inspection), with a real Windows-backed implementation and a fake used by tests to deterministically inject permission-denied, sharing-violation, and disk-full failures (§3.1, §8). This is what makes the error-recovery and conflict state machines unit-testable in `filecommand-core` without a terminal or a real fault. Rationale: §8 explicitly calls for "error injection via the fs trait seam." Alternative considered: mocking at the `std::fs` boundary via temp dirs only — rejected because disk-full and sharing-violation cannot be reliably provoked on a temp dir, so the trait seam is required for coverage.

### D3 — `\\?\` long-path abstraction, not the registry/manifest opt-in

Path handling routes every fs call through an abstraction that applies the `\\?\` (and `\\?\UNC\`) prefix as needed. The spec (§3.1) explicitly chooses this over relying on Windows 10 1607+ `LongPathsEnabled` + manifest `longPathAware`, because `\\?\` works unconditionally without a machine-wide setting. Path joining including `\\?\` prefixing is a property-test target (§8, proptest). Rationale/trade-off: `\\?\` paths bypass normalization (no relative components, forward slashes, or `.`/`..`), so the abstraction must fully canonicalize before prefixing — this is centralized in one place so callers never hand-build prefixed paths.

### D4 — Move = same-volume rename vs cross-volume copy-then-delete; delete only after verified copy

F6 Rename/Move within a volume is an instant `rename`; across volumes it is copy-then-delete, and the source is deleted only after the copy is verified (§3.1, §7). Case-only renames (`foo` → `Foo`) must succeed: the target-exists check is **file-identity-aware** (compare file identity/volume+index), not a name-string comparison, so a case-only rename is not mistaken for a self-overwrite conflict (§3.1). Rationale: directly from the spec; the identity check reuses the same metadata-identity query exposed by the fs trait (D2). Alternative considered: always copy-then-delete for uniformity — rejected because same-volume rename must be instant to preserve the NC feel and to avoid needless data movement.

### D5 — Copy preserves ADS, attributes, and timestamps; verify by integration test

Copy relies on `std::fs::copy` mapping to `CopyFileEx` on Windows, which the standard library documents as copying alternate NTFS streams, and which preserves attributes/timestamps as normal Win32 semantics (§3.1). The spec flags that the attribute/timestamp preservation is not spelled out in Rust's docs and must be **verified with an M2 integration test rather than assumed** (§3.1) — that verification is an explicit task in this change. Read-only target attributes are handled (cleared as needed before overwrite/delete) per §3.1/§7. Rationale: leans on the platform primitive where it is documented, and pins the undocumented parts with a test.

### D6 — Reparse-point semantics with cycle protection

Reparse points (symlinks, junctions) are shown with a marker; `Enter` follows them; **Delete removes the link itself, never the target's contents**; **Copy copies the link target's content by default** (NC-era behavior) with recursion-cycle protection via a visited-ID set, and recursive operations never traverse into junctions pointing inside the source tree (§7). Rationale: directly from §7; the visited-ID set uses the same file-identity query as D4. Trade-off: content-copy of link targets can duplicate data, but it matches classic NC behavior and the spec makes it the default.

### D7 — Selection lives in `panel`; totals exclude directory bytes

The selection set is per-panel state in the `panel` module (§3.1), keyed by the original `OsString`/entry identity so it survives re-sorts and in-panel navigation. `Ins` toggles the current entry and advances the cursor; `+`/`-` apply a wildcard match (using the same wildcard machinery as the listing filter) to select/deselect a group; `*` inverts. The mini-status `N files selected, X bytes` sums file sizes only — **selected directories contribute 0 bytes** (§4.1, classic NC; no directory sizing in v1). Rationale: §4.1 fixes both the wording and the 0-byte rule; keeping selection in core keeps it unit-testable (§8 "selection semantics").

### D8 — Dialogs are pure renders of core dialog state (§4.4, §4.11)

Each dialog (destination input, overwrite conflict, progress, error recovery, delete confirm, skipped-files summary) is modeled as core state and rendered by a `filecommand-tui` view, consistent with "rendering is a pure function of core state" (§3.3). Styling is fixed by §4.11: primary dialogs black-on-cyan with a double-line frame; error dialogs bright-white-on-red; input fields in bracket-and-dots style; the progress byte bar drawn with `█` (`dialog.gauge.filled`, blue on cyan) and `░` (`dialog.gauge.empty`). The destination input pre-fills with the opposite panel's path (§4.4, §5 F5/F6). The overwrite dialog shows source vs target size/date, and timestamps render through the injected `Clock`/formatting path so they are pinnable in snapshots. Rationale: keeps all dialog logic in core for unit tests and leaves the TUI as a thin renderer.

### D9 — Conflict and error resolution as explicit state machines with "All" latching

Overwrite-conflict resolution (Overwrite/Skip/Rename/Overwrite All/Skip All) and per-file error recovery (Retry/Skip/Skip All/Abort) are modeled as state machines in `fs_ops`, where the "…All" choices latch a policy that auto-resolves subsequent same-class events without re-prompting (§7). The overwrite state machine is called out as a property-test target (§8). Skipped items accumulate into a list surfaced by the end-of-job summary dialog (§4.4, §7). Rationale: §8 explicitly names "the overwrite-conflict-resolution state machine" for property testing, which requires it to be an isolated, deterministic core component.

### D10 — Snapshot the dialogs; pin time, size, locale

New dialog screens (destination input, overwrite conflict, progress with the block gauge, error recovery, delete confirm, skipped-files summary) get `insta` snapshot tests via ratatui's `TestBackend` (§8), with time pinned through the injected `Clock` trait, terminal size fixed, and locale pinned; fixture directories use fixed timestamps. Rationale: §8 mandates `TestBackend`/`insta` snapshots and the `Clock` seam precisely so timestamp-bearing dialogs render deterministically.

### D11 — Auto re-read affected panels on completion

When a job finishes (or is cancelled after partial progress), the affected panel(s) re-read automatically, reusing M1's streaming listing path (§7 refresh policy, §4.10 streaming). Selection is reconciled against the new listing (entries that vanished drop out of the set). Rationale: §7 fixes automatic re-read after FileCommand's own operations; there is no fs watcher in v1.

## Risks / Trade-offs

- **[Undocumented `CopyFileEx` attribute/timestamp preservation may not hold]** -> D5 pins it with a mandatory M2 integration test on real NTFS in CI (`windows-latest`); if it fails, fall back to explicit `SetFileAttributes`/`SetFileTime` after copy.
- **[Worker/UI reply-channel deadlock or a job that ignores Cancel]** -> Model conflict/error/cancel as explicit request→reply events (D1/D9); the worker checks a cancel flag at every file boundary and between chunk copies so Cancel is honored promptly; cover cancellation-mid-job in unit tests (§8).
- **[`\\?\` prefixing mistakes corrupt paths (relative components, forward slashes)]** -> Centralize prefixing in one abstraction (D3) and property-test path joining including `\\?\` (§8); callers never build prefixed paths by hand.
- **[Case-only rename misdetected as an overwrite conflict, or a self-overwrite silently truncates a file]** -> Identity-aware target-exists check (D4) using the fs trait's metadata-identity query, exercised with the fake fs.
- **[Reparse-point recursion cycles (junction pointing inside the source tree)]** -> Visited-ID set and no-traverse-into-junctions rule (D6), tested with fixture link/junction trees.
- **[Disk-full / sharing-violation / permission-denied are hard to provoke on real temp dirs]** -> The fake fs behind the trait seam (D2) injects them deterministically so the recovery state machine (D9) is fully covered without flaky real-disk setups.
- **[Selection drifts or is lost across re-sort/navigation/re-read]** -> Key selection by entry identity, not row index (D7), and reconcile against the fresh listing on auto re-read (D11); covered by selection-semantics unit tests.

## Open Questions

- Cross-volume move verification depth: is a size+identity check after copy sufficient before deleting the source, or should M2 add an optional content hash for the paranoid path? (Default: size + successful-copy verification per §3.1; hashing deferred unless the integration test motivates it.)
- Group select/deselect wildcard semantics: confirm the `+`/`-` dialog matches on the display name (lossy) vs the original `OsString`; the spec ties fs operations to the original `OsString` (§3.1 listing), so matching should run against the original name — to be finalized in the `selection` spec.
