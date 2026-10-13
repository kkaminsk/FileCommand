## Why

Directory-listing replies are addressed only by panel side, so a listing the user has already moved past can still pour its entries into whatever the panel shows now (audit 2026-10-12 **R-04**, High). Enter a slow directory and press Backspace before it finishes: the old worker's entries — including its own `..` — stream into the parent's listing, and its `ListingComplete` can mark the new listing complete early. Two quick Ctrl+R presses on a directory that takes a moment to read list its entries twice, because chunks are inserted whatever listing they belong to. Switching tabs mid-listing sends the old tab's entries into the newly active tab and leaves the old tab stuck at `Reading…` forever. The panel then shows files that do not exist in that directory, and F5/F6/F8 on them resolve against the *current* directory — either failing or hitting a different file with the same name. The codebase already guards Info, git, drive-label, find-file, and viewer-search replies with request ids; listings are the one async reply without one (Ctrl+U refuses to swap while streaming because of exactly this hazard).

Separately, superseded background work never stops (audit **P-10**, Low): Esc in find-file hides the dialog but the subtree walk keeps reading the whole tree and posting every match; a viewer F7 search keeps scanning a multi-GB file (holding its own mapping) after the viewer closes or a newer search starts. A search from `C:\` with a short pattern keeps a disk and a core busy long after it was dismissed.

## What Changes

- Every directory listing is tagged with a fresh request id (minted like `git_request`, via `State::next_request_id`) and recorded as the panel's current listing request. `ListingChunk`, `ListingComplete`, and `ListingFailed` carry that id; results whose id does not match the panel's current listing request are dropped before they touch any panel state.
- One generic cancellation mechanism for superseded background work: a new `Effect::CancelRequest(u64)` that the reducer emits whenever it supersedes or abandons a cancellable request, and a TUI-side registry mapping request id → `fs_ops::job::CancelFlag` (the flag that already backs job cancellation). Listing, find-file, and viewer-search workers receive the flag and stop — sending nothing further — once it is set.
- A tab that stops being active while its listing is still being read (Alt+1..9, Ctrl+W) has that read cancelled and is marked stale, so it re-reads on its next activation through the existing "Stale background tab refresh on activation" path instead of keeping a half-read listing. Ctrl+T keeps the in-progress listing flowing into the new tab, which shows the same directory, and marks the original tab stale.
- Find-file's subtree walk stops on Esc, on confirming a result while it is still running, and when a new search replaces it. A viewer F7 search stops when the viewer closes or another F7 search starts.
- Non-goal: lifting the Ctrl+U "refused while streaming" rule (being added by the in-flight `pulldown-menu-completion` change). It stays as specified.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `panel-navigation`: ADDED "Superseded directory listings are discarded and cancelled".
- `panel-tabs`: ADDED "Switching or closing a tab mid-listing never mixes listings". MODIFIED "Stale background tab refresh on activation": a tab left mid-listing is a second source of staleness, and the "no pending staleness" scenario is qualified accordingly; the other scenarios are kept verbatim.
- `find-file`: ADDED "Dismissed or superseded searches stop".
- `viewer`: ADDED "Search stops when superseded or when the viewer closes".

## Impact

- **Core (`crates/filecommand-core`):**
  - `update.rs` — `Command::ListingChunk`/`ListingComplete`/`ListingFailed` and `Effect::StartListing` gain `request: u64`; new `Effect::CancelRequest(u64)`; `State::initial` and `begin_listing_inner` mint the id; `apply_listing_event` drops mismatched ids; `update()`'s listing-failure drag clear only fires for an accepted failure; `Command::SwitchTab`/`CloseTab` handlers cancel the leaving tab's in-progress listing; `handle_find_file` (`FindFileCancel`, `FindFileConfirm`) and `handle_viewer` (`ViewerClose`, `ViewerSearchConfirm`) emit `CancelRequest`.
  - `panel.rs` — `PanelState::listing_request` and `TabData::listing_request`; `to_tab_data` marks a tab stashed mid-listing stale.
  - `listing.rs` — `find_in_subtree` and `list_dir_chunked` take a `&CancelFlag`; `viewer/search.rs` — `find_forward` takes a `&CancelFlag`.
  - Tests: `update/tests.rs` (≈60 listing-command sites move to a request-aware helper), `panel.rs`, `listing.rs`, `viewer/search.rs`, `find_file.rs`.
- **TUI (`crates/filecommand-tui`):**
  - `worker.rs` — `spawn_listing`, `spawn_find_subtree`, `spawn_viewer_search` take the request id and a `CancelFlag`, check it, and send nothing after cancellation.
  - `app.rs` — `Runtime` gains a cancel registry; `run_effects` registers flags on `StartListing`/`FindInSubtree`/`RunViewerSearch` and executes `CancelRequest`; `apply` retires a registry entry when the request's final reply passes through.
  - Tests: `worker.rs` unit tests, `tests/integration_smoke.rs`, `tests/snapshot_views.rs` call sites.
- **No persisted-format, config, or dependency changes.**

**Audit findings:** R-04, P-10

**Coordination:** `streaming-listing-performance` reworks the same listing pipeline (`spawn_listing`, `list_dir_chunked`, `apply_listing_event`) and must land **after** this change, moving the cancel check inside its streaming enumeration. `link-aware-listing` changes `StdFsReader` classification in the same worker path (no ordering constraint; rebase whichever lands second). `process-lifecycle-hardening` makes a panicking listing worker report `ListingFailed` — it must carry the request id introduced here. `resize-preserves-session-state` keeps the viewer alive across a too-small resize, so `ViewerClose` remains the only viewer exit that cancels a search. `reducer-overlay-structure` restructures the `update_impl` gates these handlers sit behind. The in-flight `reread-preserves-cursor` anchor logic runs inside `apply_listing_event` after the new id check; the in-flight `pulldown-menu-completion` Ctrl+U rule is unchanged.
