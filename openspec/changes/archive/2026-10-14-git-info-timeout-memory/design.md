# Design: git-info-timeout-memory

## Context

Verified on `build/open-proposals-2026-10` (commit `1bada6f`):

- **Spawning.** `Effect::QueryGitInfo { panel, path, request }` is built by `git_info_query_effect` (`update.rs:3840-3844`), called from `State::initial` (`update.rs:450-451`), `begin_listing_inner` (`update.rs:3833`, every navigation and re-read), and `handle_swap_panels` (`update.rs:3652`). `run_effects` calls `worker::spawn_git_info_query` (`app.rs:924`), which spawns an outer thread that calls `race_with_timeout(GIT_INFO_TIMEOUT = 3 s, || git_info::query(&path))` (`worker.rs:74-98`); `race_with_timeout` spawns an inner thread and waits on `recv_timeout`, returning `None` both on a timeout and when the inner thread dies without sending (a panic). On `None` the outer thread sends `GitInfoResolved { info: GitInfo::none() }`; after a timeout the inner thread keeps walking until libgit2 returns, and its result is dropped. Nothing records the timeout and nothing limits how many queries run at once — the audit's claim is accurate.
- **Reducer.** `Command::GitInfoResolved { panel, path, request, info }` is applied only when `p.cwd == path && p.git_request == Some(request)` (`update.rs:1264-1269`). There is no "no info" memory in `State`. `PanelState::begin_new_listing` resets `git_info` to `GitInfo::none()` and `git_request` to `None` (`panel.rs:535-536`); `handle_swap_panels` does not — each panel's `git_info` moves with it and is re-queried.
- **Query.** `git_info::query` (`git_info.rs:86-93`) calls `Repository::discover(dir)` every time, resolves the branch, then `pathspec_status` (`git_info.rs:105-150`): it canonicalizes `dir` and the workdir, strips the prefix to get `rel_dir`, and — when `rel_dir` is non-empty — sets `opts.pathspec(rel_dir with '\\' replaced by '/')`. Every status entry is then folded through `full.strip_prefix(&canon_dir)`, so an entry outside the panel directory never reaches the result map whatever the pathspec matched. At the repository root `rel_dir` is empty, no pathspec is set, and the walk covers the whole working tree; that is correct by definition (the root *is* the panel directory) and unchanged here.
- **How libgit2 1.8.1 (vendored by `libgit2-sys 0.17.0+1.8.1`) treats the pathspec.** `git_attr_fnmatch__parse` flags any unescaped `[`, `*`, or `?` (`git__iswildcard`) as `HASWILD`, treats a leading `!` as negation, and drops a pattern whose first character is `#` as a comment (`attr_file.c:729`). `pathspec_match_one` (`pathspec.c:138-171`) tries an exact compare, then `wildmatch` (no `WM_PATHNAME`, so `*` and `?` also match `/`), then a "directory prefix" fallback that is skipped when `HASWILD` is set, then a literal fallback for paths that themselves begin with `!`. Separately, `git_pathspec_prefix` (`pathspec.c:21-46`) turns the text before the first wildcard into the iterator's start/end bound, so the walk only visits paths with that string prefix. Probed against a real repository with `git2 0.19.0` on Windows (`core.ignorecase = true`), with a modified `a.txt` and an untracked `new/` in each directory:
  - `foo[1]` and `x/foo[1]` get **nothing**: `[1]` is a character class matching `1`, and the prefix fallback is skipped. On other platforms `*` and `?` in a name break the same way. This is the real bug.
  - `!old` **works today**: the pattern becomes the negation of `old`, but the literal `!`-path fallback matches `!old/a.txt` and `!old/new/`.
  - `#notes` **works today and stays scoped**: the comment-pattern is dropped, but the iterator bound `#notes` still limits the walk to paths starting with `#notes` (a sibling `#notes2` is walked too and removed by the `strip_prefix` fold). The query is not repo-wide.
  - Names with spaces or `]` work today.
- **Backslash escaping is not available on Windows.** `git2 0.19`'s `StatusOptions::pathspec` passes the string through `util::cstring_to_repo_path`, which on Windows rewrites every `\` to `/` (`fixup_windows_path`). An escaped pathspec such as `foo\[1\]/*` reaches libgit2 as `foo/[1/]/*` and matches nothing; in the probe every backslash-escaped form (`foo\[1\]/*`, `\!old/*`, `\#notes/*`, `a\ b/*`) returned no entries at all.
- **`disable_pathspec_match(true)`** (the audit's suggestion) moves the pathspec to the iterator's literal path list, which includes a directory's children only when the entry ends in `/` (`iterator.c:302-356`), and still runs directories through `pathspec_match_one` with `wildmatch` off — where the prefix fallback is skipped for a `HASWILD` pattern. Probed: `foo[1]/` with the flag set reports `foo[1]/a.txt` but drops the untracked `foo[1]/new/`.

## Goals / Non-Goals

**Goals:**

- The spec's existing "no info for the session" rule actually holds, observably and testably.
- No pile-up of status walks: at most one per panel in flight, plus at most one orphaned walk per panel per remembered timeout.
- Correct markers for directories whose names contain pattern-special characters, on Windows and elsewhere, without regressing names that already work (`!old`, `#notes`, spaces).
- The reducer stays pure; the remembered set is ordinary `State`, unit-testable in core.

**Non-Goals:**

- Cancelling a libgit2 status walk mid-walk (no API exists) or reusing `listing-request-guard`'s `CancelRequest` for git queries.
- Caching an open `Repository` per working directory or a long-lived git thread (open question).
- Changing the 3-second timeout, adding a way to retry a remembered repository within the session, or persisting the set across sessions.
- Worker panic handling in general (`process-lifecycle-hardening`); this change only keeps a panic from being mistaken for a timeout.
- `git2` feature trimming (B-04, `ci-and-build-tooling`).

## Decisions

### D1: The worker reports the timeout and the repository root; `State` remembers it

`Command::GitInfoResolved` gains `timed_out_root: Option<PathBuf>`, and `State` gains `git_no_info_roots: Vec<PathBuf>` (session-only, never persisted). The reducer records a reported root (deduplicated) **before** its stale-reply check — a timeout is evidence about the repository, not about the panel's current directory, so it is kept even when the panel has already moved on. Recording a root changes no panel field, so a stale timed-out reply still leaves the current rendering unchanged (git-info "Late result after navigation is dropped"). `git_info_query_effect` returns `Option<Effect>`: for a path under any remembered root it issues nothing, and sets the panel's `git_info` to `GitInfo::none()` and `git_request` to `None`. After `begin_new_listing` both are already reset; the explicit reset matters for `handle_swap_panels`, where a panel that resolved branch info before the timeout would otherwise keep showing it inside a "no info" repository. `State::initial`, `begin_listing_inner`, and `handle_swap_panels` handle the `None`.

*Alternative:* a TUI-side `HashSet` consulted in `run_effects` — keeps the reducer unchanged, but the "is this directory under a slow repository" decision would live outside core, invisible to `update` tests, and the reducer would mint `git_request`s that are never answered. Rejected in favor of the pure-core version.

The "Background repository detection" scenario "Query re-issued on navigation" is narrowed to directories outside a remembered repository: as written it promised a query on every navigation, which the existing "no info for the session" rule already contradicted.

### D2: Which root is remembered, spelled the way the panel spells it

`git_info::query` is split into `discover(dir) -> Option<(Repository, depth)>` and the status walk. The inner thread publishes the discovered root into an `Arc<Mutex<Option<PathBuf>>>` before starting the walk. The root is expressed in the queried path's own spelling: `canon_dir.strip_prefix(canon_workdir)` has `k` components, so the root is `dir.ancestors().nth(k)` — the same spelling the panel's later `cwd`s will use, avoiding 8.3-short-name, `\\?\`, and separator mismatches between the panel and libgit2's `C:/…/` workdir. On timeout the outer thread reports the published root, or the queried directory itself when discovery had not finished (then only that directory and its descendants are remembered). Membership (`git_info::is_under_root`) is a component-wise `starts_with`, ASCII-case-insensitive on Windows; it lives in core so both the reducer and the worker use the same test.

Only a real timeout reports a root. `race_with_timeout` distinguishes `RecvTimeoutError::Timeout` from `Disconnected` (the inner thread died without sending, i.e. panicked): a panicked query reports `GitInfo::none()` with `timed_out_root: None`, as today, so a one-off panic does not silence a repository for the session.

### D3: One query in flight per panel, newest request waits

`Runtime` owns a `GitQuerySlot` per panel side: `Mutex<{ running: bool, pending: Option<(PathBuf, u64)> }>`. `Effect::QueryGitInfo` either starts the slot's runner thread (when idle) or replaces `pending` (when running). The runner loops: run one query through `race_with_timeout`, send `GitInfoResolved`, then take `pending` and continue, or mark the slot idle and exit. A replaced pending request is never started — its id is already superseded in `PanelState::git_request`, so its reply would be dropped anyway. When the query just reported a timeout with root `R`, the runner also discards a pending request whose directory is under `R` (`git_info::is_under_root`): it was queued before the reducer learned about `R`, and starting it would begin a second walk of the slow repository — exactly the pile-up this change removes. Its panel shows no git info there, which is what the reducer would decide anyway once `R` is recorded.

The slot is released when the reply is **sent**, not when an orphaned libgit2 walk eventually returns: holding the slot for a walk that may take minutes would starve git info for every other repository on that panel, and D1 already ensures no new query enters the slow repository. The `running` flag is cleared by a drop guard, so a panic in the runner cannot leave the slot marked busy (which would end git info for that panel for the session).

*Alternatives:* drop the new request while one runs (the audit's "drop a query if one is already running") — the panel would never get git info for the directory it ended up in; rejected. A long-lived git thread per panel that also caches `Repository` handles — larger change, see Open Questions.

### D4: Literal pathspec by bracket-quoting wildcard characters, plus `/*`

`pathspec_status` builds its single pathspec with `literal_pathspec(rel_dir)`, joining `rel_dir`'s components with `/` (instead of replacing `\` in the whole string):

- If the relative path contains none of libgit2's wildcard characters (`[`, `*`, `?`, and on non-Windows a `\`, which libgit2 reads as an escape), it is passed **unchanged** — today's form, which the probe shows already handles `!old`, `#notes`, spaces, and `]` with the walk scoped by the iterator prefix.
- Otherwise each `[`, `*`, and `?` becomes a one-character bracket class (`[[]`, `[*]`, `[?]`), a non-Windows `\` becomes `?`, a leading `!` or `#` becomes `?`, and `/*` is appended. The bracket classes match the literal character without needing a backslash (which git2-rs rewrites to `/` on Windows); `/*` is needed because the prefix fallback is off for any wildcard pattern, and since `*` also matches `/` it covers files at any depth and unexpanded untracked subdirectories (`new/`). A leading `!` or `#` would otherwise make the pattern a negation or a comment; `?` in its place can also match a sibling that differs only in that first character, and such entries are removed by `pathspec_status`'s existing `strip_prefix(&canon_dir)` fold.

Probed on Windows: `foo[[]1]/*`, `x/foo[[]1]/*`, `[[]x]/*`, `x/!b[[]2]/*`, `?b[[]2]/*` (for `!b[2]`), and `?c[[]3]/*` (for `#c[3]`) each report the directory's modified `a.txt` and untracked `new/` and nothing else after the fold. Case folding is unchanged (libgit2 applies `core.ignorecase` through `WM_CASEFOLD`).

The walk stays scoped by the iterator prefix up to the first quoted character (`foo` for `foo[1]`, `x/foo` for `x/foo[1]`). For a top-level name that starts with a pattern character, or with `!`/`#` and also contains one, the prefix is empty and libgit2 walks the whole working tree for that directory; the reported markers are still correct.

*Alternatives:* backslash escaping (`foo\[1\]/*`, the original plan) — works on Unix but matches nothing on Windows because git2-rs rewrites `\` to `/`; rejected. `disable_pathspec_match(true)` with a trailing `/` (the audit's suggestion) — literal for files, but untracked subdirectories of a directory whose name contains `[`, `*`, or `?` lose their `?` marker (Context); rejected. Calling `git_status_list_new` through `libgit2-sys` directly to bypass the rewrite — needs a new direct dependency and `unsafe` for a rare case; rejected.

## Risks / Trade-offs

- [A repository that was only slow once (cold cache, momentary network stall) shows no git info for the rest of the session] → this is the existing spec's rule; restarting FileCommand retries.
- [Remembering the queried directory when discovery itself timed out can miss sibling directories of the same slow repository] → those get one query each, bounded by distinct directories visited; each timeout adds its own root.
- [An orphaned walk still runs to completion per timeout] → bounded to one per panel per remembered root, since no new or queued query enters it.
- [The quoting relies on libgit2's `wildmatch` bracket-class semantics and on git2-rs's Windows backslash rewrite] → covered by real-repository tests for `foo[1]`, a nested `x/foo[1]`, `[x]`, `!b[2]`, `#c[3]`, `!old`, `#notes`, and a name with a space on every platform, and `a*b`, `what?`, and `back\slash` on non-Windows (Windows forbids those characters in names); a `git2` upgrade that changes either behavior fails these tests.
- [A top-level directory whose name starts with a pattern character is walked repo-wide] → rare; correctness is unaffected and it is no worse than the repository root, which is always walked repo-wide.

## Migration Plan

None — no persisted data or configuration changes.

## Open Questions

- Should a long-lived git thread per panel cache an open `Repository` per working directory, avoiding `Repository::discover` and index loading on every navigation? Worth measuring on a large monorepo after this change; out of scope here.
