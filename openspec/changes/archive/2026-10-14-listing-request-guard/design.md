# Design: listing-request-guard

## Context

Verified on `build/open-proposals-2026-10` (commit `1bada6f`):

- **Listing replies have no generation key.** `Command::ListingChunk { panel, entries }`, `ListingComplete { panel, total }`, and `ListingFailed { panel, message }` (`update.rs:708-710`) and `Effect::StartListing { panel, path }` (`update.rs:1006`) carry only a `PanelSide`. `update_impl` folds them in before any phase or overlay gating (`update.rs:1241-1244`), and `apply_listing_event` (`update.rs:3846-3911`) inserts into whatever directory and tab that side shows *now*. `StartListing` is emitted by `State::initial` (`update.rs:444-445`) and by `begin_listing_inner` (`update.rs:3813-3839`), which every navigation, re-read, job-completion refresh, stale-tab refresh, and the Tree-mode preview of the opposite panel go through.
- **Workers are never told to stop.** `worker::spawn_listing` (`worker.rs:178-203`) sends a synthetic `..` chunk first when the directory has a parent, then whatever `list_dir_chunked` produces, then `ListingComplete`/`ListingFailed`. Nothing can stop it.
- **Tabs.** `Command::SwitchTab`/`CloseTab` (`update.rs:1557-1579`) have no streaming guard. `PanelState::to_tab_data` (`panel.rs:764-789`) stashes the active tab — including `progress: Streaming` — with `stale: false`, so the stashed tab is never re-read and stays at `Reading…` forever, while its worker's chunks land in the newly active tab. `switch_tab`/`close_tab` return the target tab's `stale` flag and the reducer then re-reads it through `begin_listing_inner(.., true)` without a frecency visit (panel-tabs "Stale background tab refresh on activation").
- **Ctrl+U** (`handle_swap_panels`, `update.rs:3632-3638`) refuses while either side streams — the hazard was known and patched at that one call site.
- **Find-file** already ignores stale replies (`FindFileState::request`, `push_match`/`mark_done` compare ids). `FindFileCancel` just sets `state.find_file = None` (`update.rs:3507`); `FindFileConfirm` takes it (`update.rs:3509`). The walk (`worker::spawn_find_subtree`, `worker.rs:169-176` → `listing::find_in_subtree`, `listing.rs:566-583`) has no stop signal and keeps posting every match into the unbounded channel. Re-submitting inside one dialog is impossible (Enter maps to `FindFileConfirm` once a search is in flight), so a "new search" means Esc, reopen, submit.
- **Viewer search** already ignores stale replies (`ViewerState::search_request`, `update.rs:2154`). `ViewerClose` returns immediately (`update.rs:2110`); `spawn_viewer_search` (`worker.rs:133-145`) opens its own `ByteSource` and `find_forward` (`viewer/search.rs:22-49`) scans to the end of the file. A second F7 while one runs mints a new id but the old scan continues.
- **The flag already exists.** `fs_ops::job::CancelFlag` (`fs_ops/job.rs:94-110`) is a cloneable `Arc<AtomicBool>`; the TUI keeps one per running job in `Runtime::active_job` (`worker::JobHandle`).

## Goals / Non-Goals

**Goals:**

- No listing result ever lands in a directory, tab, or listing generation other than the one that requested it.
- One cancellation mechanism shared by listings, find-file walks, and viewer searches, with the supersession decision made where the request ids already live — the reducer.
- A tab left mid-listing recovers by itself on its next activation, reusing the existing stale-refresh path.
- Core stays pure: no flags, threads, or atomics enter `State`.

**Non-Goals:**

- Lifting Ctrl+U's "refused while streaming" rule (in-flight `pulldown-menu-completion` adds it; it stays).
- Making enumeration itself interruptible mid-`read_dir` — that needs the streaming reader from `streaming-listing-performance` (P-01); this change checks the flag between chunks.
- Cancelling git-info queries (libgit2 status walks have no cancel hook; `git-info-timeout-memory` handles their pile-up), Info queries, drive-label fetches, or Tree-node expansion (all bounded, single-directory work).
- Error-message content (B-06, `error-diagnostics`), worker panic reporting (R-12, `process-lifecycle-hardening`), streaming performance (P-01..P-04).

## Decisions

### D1: A per-listing request id on `PanelState`, minted like `git_request`

`begin_listing_inner` and `State::initial` mint `state.next_request_id()` for each listing, store it in a new `PanelState::listing_request: Option<u64>`, and put it on `Effect::StartListing { panel, path, request }`. The worker echoes it on every `ListingChunk`, `ListingComplete`, and `ListingFailed`. `listing_request` is part of `TabData` (stash/restore beside `git_request`), and moves with the panel on Ctrl+U like every other field.

*Alternatives:* key on the directory path — rejected: a re-read of the same directory (double Ctrl+R, job-completion refresh during a manual re-read) must still distinguish generations. A per-panel generation counter separate from `next_request_id` — rejected: the shared counter is already the house pattern (Info, git, drive labels, find-file, viewer search) and ids from it are globally unique, which the TUI's cancel registry (D3) relies on.

### D2: Drop mismatches first, before any mutation

`apply_listing_event` begins with `if state.panel(panel).listing_request != Some(request) { return; }`, ahead of the entry insert, the progress count, the in-flight `reread-preserves-cursor` anchor application, the find-file `pending_cursor_target` settle, and the selection reconcile. `update()`'s post-condition that clears a drag on `ListingFailed` (`update.rs:1141-1153`) only fires when the failure was accepted, so a stale failure from a directory the user left cannot cancel a drag in progress elsewhere. A completed listing keeps its `listing_request` (late replies still mismatch nothing, because the worker sends nothing after completion), so no clearing on completion is needed.

### D3: One cancel mechanism — `Effect::CancelRequest(u64)` + a TUI registry of `CancelFlag`s

The reducer decides supersession (it mints and compares ids), so it emits `Effect::CancelRequest(id)`; the TUI owns the flags, exactly as it owns `JobHandle`. `Runtime` gains a small `CancelRegistry` (`HashMap<u64, CancelFlag>` behind `register`, `cancel`, `retire`):

- `run_effects` creates and registers a fresh flag when executing `StartListing`, `FindInSubtree`, and `RunViewerSearch`, passing a clone to the worker.
- `CancelRequest(id)` sets the flag and removes the entry; an unknown id (already finished, never cancellable) is a silent no-op, so the reducer never needs to know whether a worker has finished — this matters for the viewer, whose `search_request` stays `Some` after its result arrives.
- `apply` retires the entry when the request's final reply passes through it (`ListingComplete`/`ListingFailed`, `FindFileSearchDone`, `ViewerSearchResult`), the same pre-`update` interception point `apply` already uses for `ViewerClose`.

*Alternatives:* a `CancelFlag` inside `State` — rejected: an `Arc<AtomicBool>` breaks the pure-core contract (application-shell "Pure core update function": equal states must yield equal results) and the core/tui boundary. A per-panel "current request" atomic the worker polls — rejected: listing-only, and find-file/viewer would need their own. Per-request channels that the worker notices closing — rejected: all workers share the one `Sender<Command>` by design (application-shell "Single-threaded UI event loop with worker threads").

### D4: Where the reducer emits `CancelRequest`

| Site | Cancels | When |
|---|---|---|
| `begin_listing_inner` | the panel's previous `listing_request` | the previous listing is still `Streaming` |
| `SwitchTab` | the leaving tab's `listing_request` | the leaving tab is `Streaming` |
| `CloseTab` | the closed tab's `listing_request` | the closed tab is `Streaming` |
| `FindFileCancel`, `FindFileConfirm` | `FindFileState::request` | a search was submitted and is not `done` |
| `ViewerClose`, `ViewerSearchConfirm` | the previous `search_request` | one was ever issued in this session |

`OpenTab` (Ctrl+T) emits nothing: the new tab inherits the active tab's state, including `listing_request`, so the in-progress listing keeps filling it — it is the same directory. Ctrl+U cannot reach a streaming panel (refused).

### D5: Workers check the flag and go silent

- `spawn_listing` checks before sending the `..` chunk, before each chunk, and before the final `ListingComplete`/`ListingFailed`; `list_dir_chunked` takes `&CancelFlag` and stops slicing once it is set. Until `streaming-listing-performance` replaces `read_dir`'s collect-everything call with a streaming reader, a superseded listing still finishes its one `read_dir` call but sends nothing afterwards; after that change the check moves inside the enumeration callback.
- `find_in_subtree` takes `&CancelFlag` and checks before each directory read and between entries.
- `find_forward` takes `&CancelFlag`, checks before each chunk read, and returns `None` once set; `spawn_viewer_search` then sends no reply and drops its `ByteSource`, releasing its own mapping of the file.

A cancelled worker sends no terminal message: its id is already superseded in core, so any message would be dropped anyway, and staying silent keeps the channel quiet.

### D6: Tabs reuse the stale-refresh path

`to_tab_data` stamps `stale: self.progress.is_streaming()` instead of a constant `false`. Every path that stashes the active tab goes through it (`open_tab`, `close_tab`, `switch_tab` via `full_tab_list`), so a tab stashed mid-listing is re-read when next activated: `switch_tab`/`close_tab` already return the flag and the reducer already calls `begin_listing_inner(.., true)` — a non-navigation re-read (no frecency visit, no history persist). While stashed, the tab's chunks are dropped by D2 because the inline `listing_request` belongs to the newly active tab.

This widens what "stale" means, so the panel-tabs delta MODIFIES "Stale background tab refresh on activation": its description names the second source of staleness, and its scenario "Switching to a tab with no pending staleness is unchanged" gains the condition "and whose listing had finished being read when it was left". Without that, the scenario (a tab untouched by any job activates from cache) would contradict "Returning to an interrupted tab re-reads it". The other three scenarios are kept verbatim. No other new or in-flight change modifies that requirement.

**Keeping an interrupted re-read's anchor.** The in-flight `reread-preserves-cursor` requires a stale tab's activation refresh to restore the cursor, and its design D5 stashes `reread_anchor` in `TabData` so "a tab switched away mid-re-read keeps its anchor for when its listing completes". Under this change that listing is cancelled and never completes; the stale refresh replaces it. But `capture_reread_anchor` runs first in `begin_listing_inner` and, because the stashed tab's progress is `Streaming`, it *clears* the anchor (it only anchors to a `Complete` listing), so the refreshed tab would jump to the top. The stale-tab path therefore calls `begin_listing_inner(.., capture_anchor: false)` when the activated tab is still `Streaming`, and `true` (today's call) otherwise. `begin_new_listing` keeps an existing anchor for the same `cwd`, so:

- a tab interrupted mid-re-read keeps the anchor that re-read captured from its last complete listing, and the refresh restores that position;
- a tab interrupted mid-navigation has no anchor (navigation cleared it), so it starts from the top, which is correct for a directory that never finished listing.

`capture_reread_anchor`, `apply_reread_anchor`, and every other caller are unchanged, so `reread-preserves-cursor`'s own behavior (including its accepted jump-to-top for a Ctrl+R issued while the previous listing still streams) is untouched.

### D7: Ctrl+U rule unchanged

With D1/D2 a mid-stream swap could no longer misdeliver chunks, but lifting the refusal is a behavior change owned by `pulldown-menu-completion`'s requirement. Only `handle_swap_panels`' doc comment is updated (it currently cites the missing request id as the reason).

### D8: Test ergonomics

About 60 test sites in `update/tests.rs` construct listing commands directly. A helper `listing_chunk(state, side, entries)` (and `listing_complete`, `listing_failed`) reads `state.panel(side).listing_request` so existing tests keep their meaning; new tests construct mismatched ids explicitly.

## Risks / Trade-offs

- [A registry entry outlives a worker that dies without a final reply] → bounded and small; `process-lifecycle-hardening` turns a listing-worker panic into `ListingFailed { request }`, which retires the entry. Find-file and viewer workers always send their final reply unless cancelled (and a cancel removes the entry).
- [Before streaming lands, a superseded listing of a huge directory still finishes its `read_dir` in the background] → nothing reaches the UI and it sends nothing; the cost is one wasted enumeration, removed by `streaming-listing-performance`.
- [Cancelling the closed or left tab's listing loses work already read] → intended: the tab re-reads on activation, and a half-read listing is never shown as if complete.
- [Mechanical churn in tests] → contained by the D8 helpers; assertions about effects match `Effect::StartListing { panel, path, .. }`.

## Migration Plan

None — no persisted data or configuration changes. The change is internal plumbing plus the observable fixes in the specs.

## Open Questions

None.
