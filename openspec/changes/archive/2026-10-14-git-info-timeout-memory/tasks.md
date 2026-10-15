# Tasks: git-info-timeout-memory

## 1. Literal pathspec (`crates/filecommand-core/src/git_info.rs`)

- [x] 1.1 Add `literal_pathspec(rel: &Path) -> String`: join components with `/`; if no component contains `[`, `*`, `?` (or, on non-Windows, `\`), return the joined path unchanged; otherwise quote `[`→`[[]`, `*`→`[*]`, `?`→`[?]`, non-Windows `\`→`?`, a leading `!` or `#`→`?`, and append `/*`. Unit tests for each rule, including that `!old`, `#notes`, `a b`, and `a]b` are returned unchanged and that no output contains a `\` (design D4)
- [x] 1.2 `pathspec_status`: when `rel_dir` is non-empty, set the single pathspec `literal_pathspec(rel_dir)`; the repository-root case (no pathspec) and the `strip_prefix(&canon_dir)` fold are unchanged (git-info "Pathspec-scoped status queries"; design D4)
- [x] 1.3 Real-repository tests, each directory holding a modified tracked `a.txt` and an untracked subdirectory `new` containing a file: `foo[1]` → `a.txt` `M`, `new` `?` (git-info "Directory names are matched literally"); `src/foo[1]` beside a modified `src/foo1` → only `src/foo[1]`'s entries ("Pattern characters in a nested directory name"); `[x]`, `!b[2]` beside a modified `Xb[2]`, and `#c[3]` → their own markers only; `!old` → `M` ("A leading exclamation mark is not a negation"); top-level `#notes` beside a modified `outside.txt` → markers present and `outside.txt` not reported ("A leading hash does not widen the query"); a name with a space; `a*b`, `what?`, and `back\slash` on non-Windows only
- [x] 1.4 Confirm `status_is_scoped_to_the_panel_directory_via_pathspec` and `untracked_directory_is_marked_but_its_contents_are_not_enumerated` pass unchanged (git-info "Status limited to panel directory", "Untracked directory not expanded")

## 2. Root discovery (`git_info.rs`)

- [x] 2.1 Split `query` into `discover(dir)` (repository plus the number of components between the workdir and `dir`) and the branch/status step, keeping `query(dir)` as their composition for existing callers (design D2)
- [x] 2.2 Add `root_in_dir_spelling(dir, depth)` = `dir.ancestors().nth(depth)` with tests for a subdirectory, the root itself, and an 8.3/long-name pair on Windows (design D2)
- [x] 2.3 Add `is_under_root(path, root)` — component-wise `starts_with`, ASCII-case-insensitive on Windows — with tests for equal paths, a descendant, a sibling sharing a string prefix (`D:\mono2` is not under `D:\mono`), and a Windows-only case-difference test (design D2)

## 3. Reducer memory (`crates/filecommand-core/src/update.rs`)

- [x] 3.1 Add `timed_out_root: Option<PathBuf>` to `Command::GitInfoResolved` and `git_no_info_roots: Vec<PathBuf>` to `State` (session-only) (design D1)
- [x] 3.2 `GitInfoResolved` handler: record `timed_out_root` (deduplicated) before the stale-reply check, then apply `info` only when `cwd` and `git_request` match, as today (git-info "Timeout recorded after the panel moved on")
- [x] 3.3 `git_info_query_effect` returns `Option<Effect>`; under a remembered root (`git_info::is_under_root`) it issues nothing and sets the panel's `git_info` to `GitInfo::none()` and `git_request` to `None`; update `State::initial`, `begin_listing_inner`, `handle_swap_panels` (git-info "Background repository detection"; design D1)
- [x] 3.4 `update/tests.rs`: timed-out reply for `D:\mono\src` with root `D:\mono` → navigation to `D:\mono\docs` and `RereadPanel` there emit no `QueryGitInfo` and leave no git info (git-info "Timed-out repository is not queried again", "Query re-issued on navigation"); navigation to `C:\work\app` emits one ("Other repositories are still queried"); a stale timed-out reply still records the root and leaves the current panel unchanged ("Timeout recorded after the panel moved on", "Late result after navigation is dropped"); a root equal to the queried directory covers its descendants only ("Repository not located before the timeout"); Ctrl+U with the other panel showing resolved info inside a remembered root issues no query for it and clears its git info ("Swapping a panel into a timed-out repository")
- [x] 3.5 Confirm the existing stale-reply tests pass unchanged (git-info "Late result after navigation is dropped", "Timeout degrades silently")

## 4. One query per panel (`crates/filecommand-tui/src/worker.rs`, `app.rs`)

- [x] 4.1 Add `GitQuerySlot` (`Mutex<{ running, pending }>`) per panel side on `Runtime`; `Effect::QueryGitInfo` starts the slot's runner when idle or replaces `pending` when running; the runner clears `running` through a drop guard (design D3)
- [x] 4.2 `race_with_timeout` reports timeout and disconnection separately; only a timeout yields `timed_out_root` (the published root, or the queried directory when none was published); a disconnected (panicked) query reports `GitInfo::none()` with `timed_out_root: None` (design D2)
- [x] 4.3 Runner loop: run `discover` + status under `race_with_timeout` with the discovered root published to a shared cell; send `GitInfoResolved`; after a timeout with root `R`, discard a pending request under `R`; then take `pending` or mark the slot idle (design D3)
- [x] 4.4 Make the runner generic over the query function so tests can inject a blocking fake; tests: a second and third request while the first runs and completes in time → only the third runs after the first reports, the second never starts (git-info "Rapid navigation does not pile up queries"); a pending request under the root of a query that then times out → never started ("Waiting query inside the timed-out repository is not started"); a fake that blocks past the timeout after publishing a root → `timed_out_root` is that root; one that blocks before publishing → the queried directory; one that panics → `timed_out_root: None` and the slot is idle afterwards
- [x] 4.5 Keep `race_with_timeout`'s existing tests passing

## 5. Verify

- [x] 5.1 `cargo test --workspace` green
- [x] 5.2 `cargo clippy --workspace --all-targets` with no new warnings
- [x] 5.3 `openspec validate git-info-timeout-memory --strict` clean
- [ ] 5.4 Manual (Windows): in a large repository on a network share, navigate repeatedly after the first timeout → no further status walks (Task Manager thread count and disk activity stay flat); a directory named `foo[1]` inside a repository shows its `M` and `?` markers
