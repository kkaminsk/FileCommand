## Why

The git-info spec already says a status query that times out marks its repository "no info" for the rest of the session, but the code never remembers it (audit 2026-10-12 **P-07**, Medium; §6 "Timeout marks the repo 'no info' for the session": **DRIFT**). Every navigation spawns two threads — `spawn_git_info_query`'s outer thread and `race_with_timeout`'s inner one — and after the 3-second timeout the inner libgit2 status walk keeps running because it cannot be cancelled. The next navigation inside the same huge repository or SMB share starts another full walk, and rapid navigation piles up concurrent, uncancellable walks that compete for I/O and make every later query slower still. Separately, the panel directory is passed to libgit2 as a wildcard pattern, so a directory whose name contains `[` (for example `foo[1]`) — or, outside Windows, `*` or `?` — silently gets no status markers. (Names with a leading `!` or `#`, spaces, or `]` already work today; they are pinned by scenarios so the fix does not regress them.)

## What Changes

- A timed-out query's repository is remembered for the session: the worker reports the timeout together with the repository's working-directory root (or, if the repository could not even be located in time, the queried directory), the reducer stores it in `State`, and no further git query is issued for any directory under a remembered root — the panel simply shows no git indicators there, silently, as the spec already requires.
- At most one git status query runs per panel at a time: a navigation while one is running queues the newest request, replacing any older queued one, and starts it when the running query reports; superseded requests are never started, and neither is a queued request for a directory inside the repository the running query has just timed out on.
- Directory names are matched literally in the status pathspec: `[`, `*`, and `?` in a directory name are quoted as one-character bracket classes instead of being interpreted as wildcards (backslash escaping cannot be used, because `git2` rewrites `\` to `/` in pathspecs on Windows). Paths without those characters are passed exactly as today.
- Unchanged: the 3-second timeout, silent degradation, stale-result discarding via `git_request`, the repo-wide walk at the repository root (pathspec-scoping applies below the root, by definition).
- Non-goal: caching an open `Repository` per working directory on a long-lived git thread (left as an open question in design.md).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `git-info`: MODIFIED "Background repository detection" (no query is issued inside a repository marked "no info"), MODIFIED "Silent absence on timeout and stale-result discarding" (the remembered-repository rule made explicit and testable; one in-flight query per panel), MODIFIED "Pathspec-scoped status queries" (directory names matched literally).

## Impact

- **Core (`crates/filecommand-core`):**
  - `git_info.rs` — `query` split so repository discovery can publish the working-directory root before the status walk; `is_under_root` (shared by reducer and worker); a literal-pathspec builder (`literal_pathspec`) used by `pathspec_status`; real-repository tests with directories named `foo[1]`, `x/foo[1]`, `[x]`, `!b[2]`, `#c[3]`, `!old`, `#notes`, and one containing a space.
  - `update.rs` — `Command::GitInfoResolved` gains `timed_out_root: Option<PathBuf>`; `State` gains a session-only set of "no info" roots; the `GitInfoResolved` handler records a root even when the reply itself is stale; `git_info_query_effect` (called from `State::initial`, `begin_listing_inner`, `handle_swap_panels`) issues no query for a path under a remembered root and clears that panel's git info.
  - Tests in `update/tests.rs`.
- **TUI (`crates/filecommand-tui`):** `worker.rs` — `spawn_git_info_query` becomes a per-panel single-slot runner (a running flag plus one pending request) owned by `Runtime`; `race_with_timeout` tells a timeout apart from a panicked inner thread, and its timeout path reports the discovered root; `app.rs` `run_effects` routes `Effect::QueryGitInfo` through the panel's slot.
- **No persisted-format, config, or dependency changes.** The remembered set lives only for the session.

**Audit findings:** P-07

**Coordination:** `listing-request-guard` introduces a generic `Effect::CancelRequest` + `CancelFlag` registry; it is deliberately not used here because a libgit2 status walk cannot be cancelled mid-walk — the single-slot runner and the remembered set bound the waste instead. `ci-and-build-tooling` (B-04) switches `git2` to `default-features = false`; nothing here depends on the default features. `idle-aware-event-loop` counts live worker threads to decide how often the loop wakes; the git slot runner's thread must hold a worker token while it can still send (the orphaned inner walk sends nothing to the UI channel and needs none). `process-lifecycle-hardening` wraps every `spawn_*` body in `catch_unwind` with `GitInfoResolved { info: GitInfo::none() }` as the git panic reply; whichever lands second keeps that reply inside the slot runner, with `timed_out_root: None`, and keeps the slot's drop guard. `reducer-overlay-structure` moves `git_info_query_effect` into `update/listing.rs`; whichever lands second rebases onto the other. No landing order is required.
