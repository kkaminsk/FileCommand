//! Background git repository detection: enclosing-repo lookup, branch
//! resolution, and pathspec-scoped per-entry status via `git2` (libgit2).
//!
//! [`query`] is the whole blocking operation as one call — repo discovery,
//! branch resolution, and status — designed to be run on a dedicated worker
//! thread that `filecommand-tui` owns (core never spawns threads itself, and
//! libgit2 status calls are blocking and not cancellable mid-call; design
//! D3). It is split into [`discover`] + [`query_in_repo`] so the worker can
//! publish the repository's root (via [`root_in_dir_spelling`]) before the
//! potentially slow status walk begins, which is what lets a timed-out query
//! mark its repository "no info" for the rest of the session
//! (git-info-timeout-memory). The reducer (`update.rs`) guards every reply
//! with a directory +
//! generation key (`PanelState::git_request`) via `Command::GitInfoResolved`,
//! so a result for a directory/generation the panel has since moved past is
//! dropped rather than applied — the same staleness-guard shape as
//! `PanelState::info_request` (git-info "Silent absence on timeout and
//! stale-result discarding").

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use git2::{Repository, Status, StatusOptions};

/// One file's reported git status, mapped to its single-cell marker glyph
/// and `panel.git.*` role (git-info "Per-file status marker column").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// Working-tree changes not yet staged. Takes precedence over `Staged`
    /// when an entry has both index and working-tree changes, since the
    /// working tree is what differs from what would actually be committed.
    Modified,
    /// Not tracked by git at all.
    Untracked,
    /// Staged (added to the index) with no further working-tree changes.
    Staged,
}

impl FileStatus {
    /// The single-cell CP437-heritage marker glyph for this status
    /// (git-info "Per-file status marker column").
    pub fn marker(self) -> char {
        match self {
            FileStatus::Modified => 'M',
            FileStatus::Untracked => '?',
            FileStatus::Staged => '+',
        }
    }

    /// Precedence when multiple statuses roll up onto the same top-level
    /// entry (e.g. two modified files inside the same subdirectory): higher
    /// wins. `Modified` > `Staged` > `Untracked`.
    fn rank(self) -> u8 {
        match self {
            FileStatus::Modified => 2,
            FileStatus::Staged => 1,
            FileStatus::Untracked => 0,
        }
    }
}

/// The resolved git info for one panel directory: `branch` is `None` when
/// the directory lies outside any repository (or a query has not resolved
/// yet — the two render identically per git-info "Single-reflow appearance
/// with nothing reserved while pending"); `statuses` maps a panel entry's
/// original file name to its rolled-up status.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitInfo {
    pub branch: Option<String>,
    pub statuses: HashMap<OsString, FileStatus>,
}

impl GitInfo {
    /// The "outside any repository" / "not yet resolved" value — no branch
    /// suffix, no marker column (git-info "Directory outside any
    /// repository").
    pub fn none() -> GitInfo {
        GitInfo::default()
    }

    pub fn is_repo(&self) -> bool {
        self.branch.is_some()
    }
}

/// Detect the repository enclosing `dir`, resolve its branch name, and
/// compute pathspec-scoped status for `dir`'s immediate entries. Blocking —
/// callers run this on a dedicated worker thread (git-info "Background
/// repository detection"). The composition of [`discover`] and
/// [`query_in_repo`], kept for callers that need the whole lookup as one
/// call.
pub fn query(dir: &Path) -> GitInfo {
    let Some((repo, _depth)) = discover(dir) else {
        return GitInfo::none();
    };
    query_in_repo(&repo, dir)
}

/// The discovery half of [`query`], split out so a caller that is about to
/// start the (uncancellable, potentially slow) status walk can first learn
/// *which* repository it is walking: the returned depth is the number of
/// path components between the repository's working directory and `dir`, so
/// [`root_in_dir_spelling`] can express the root in `dir`'s own spelling
/// (git-info-timeout-memory design D2). `None` when `dir` is not inside any
/// repository — the same condition [`query`] reports as [`GitInfo::none`].
///
/// The depth is computed on canonicalized spellings for the same reason
/// [`pathspec_status`] compares them (8.3 short-name vs long-name forms);
/// when there is no working directory (a bare repository) or the comparison
/// fails, it falls back to 0, i.e. "the queried directory itself".
pub fn discover(dir: &Path) -> Option<(Repository, usize)> {
    let repo = Repository::discover(dir).ok()?;
    let Some(workdir) = repo.workdir() else {
        return Some((repo, 0));
    };
    let canon_dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let canon_workdir = std::fs::canonicalize(workdir).unwrap_or_else(|_| workdir.to_path_buf());
    let depth = canon_dir.strip_prefix(&canon_workdir).map(|rel| rel.components().count()).unwrap_or(0);
    Some((repo, depth))
}

/// The branch-resolution and status half of [`query`] for an already
/// discovered repository (git-info "Background repository detection").
pub fn query_in_repo(repo: &Repository, dir: &Path) -> GitInfo {
    let branch = resolve_branch(repo);
    let statuses = pathspec_status(repo, dir);
    GitInfo { branch, statuses }
}

/// The repository root `depth` components above `dir`, spelled with `dir`'s
/// own components rather than libgit2's workdir form — the panel's later
/// `cwd`s share that spelling, so membership tests ([`is_under_root`]) never
/// trip over 8.3-short-name, `\\?\`, or separator differences between what
/// the panel holds and what libgit2 reports (git-info-timeout-memory design
/// D2). `None` when `dir` has fewer ancestors than `depth` (callers fall
/// back to `dir` itself).
pub fn root_in_dir_spelling(dir: &Path, depth: usize) -> Option<PathBuf> {
    dir.ancestors().nth(depth).map(Path::to_path_buf)
}

/// Whether `path` is `root` itself or one of its descendants — a
/// component-wise prefix test, so a sibling that merely shares a string
/// prefix (`D:\mono2` vs `D:\mono`) is not "under" the root. Comparison is
/// ASCII-case-insensitive on Windows, where the filesystem is, and exact
/// elsewhere. Shared by the reducer (which remembers timed-out repositories
/// for the session) and the TUI's git worker (which drops queued queries
/// inside them), so both apply the same membership test
/// (git-info-timeout-memory design D1/D2).
pub fn is_under_root(path: &Path, root: &Path) -> bool {
    let mut path_components = path.components();
    for root_component in root.components() {
        let Some(path_component) = path_components.next() else {
            return false;
        };
        if !component_matches(path_component.as_os_str(), root_component.as_os_str()) {
            return false;
        }
    }
    true
}

#[cfg(windows)]
fn component_matches(path: &std::ffi::OsStr, root: &std::ffi::OsStr) -> bool {
    path.eq_ignore_ascii_case(root)
}

#[cfg(not(windows))]
fn component_matches(path: &std::ffi::OsStr, root: &std::ffi::OsStr) -> bool {
    path == root
}

/// The current branch's short name (e.g. `main`), or `None` for an
/// unborn/detached HEAD.
fn resolve_branch(repo: &Repository) -> Option<String> {
    let head = repo.head().ok()?;
    head.shorthand().map(str::to_string)
}

/// Status for `dir`'s immediate entries only, scoped to `dir` via a
/// pathspec rather than querying the whole repository, with untracked
/// directories reported as one entry rather than having their contents
/// individually enumerated (git-info "Pathspec-scoped status queries").
fn pathspec_status(repo: &Repository, dir: &Path) -> HashMap<OsString, FileStatus> {
    let mut result = HashMap::new();
    let Some(workdir) = repo.workdir() else {
        // A bare repository has no working tree to report status for.
        return result;
    };
    // Compare canonical spellings below: `dir` may reach us in a different
    // but equivalent form than the workdir libgit2 reports (Windows 8.3
    // short-name vs long-name paths are the classic case), and a raw
    // `strip_prefix` mismatch would silently drop every status entry.
    let canon_dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let canon_workdir = std::fs::canonicalize(workdir).unwrap_or_else(|_| workdir.to_path_buf());
    let Ok(rel_dir) = canon_dir.strip_prefix(&canon_workdir) else {
        return result;
    };

    let mut opts = StatusOptions::new();
    opts.include_untracked(true);
    // Report an untracked directory as a single entry rather than walking
    // into it (git-info "Untracked directory not expanded").
    opts.recurse_untracked_dirs(false);
    if !rel_dir.as_os_str().is_empty() {
        // The panel directory is matched literally: a name containing `[`,
        // `*`, or `?` would otherwise be read as pathspec pattern syntax
        // and silently match nothing (git-info "Directory names are matched
        // literally"). At the repository root `rel_dir` is empty, no
        // pathspec is set, and the walk covers the whole working tree —
        // correct by definition, since the root *is* the panel directory.
        opts.pathspec(literal_pathspec(rel_dir));
    }

    let Ok(statuses) = repo.statuses(Some(&mut opts)) else {
        return result;
    };
    for entry in statuses.iter() {
        let Some(rel_path) = entry.path() else { continue };
        let full = canon_workdir.join(rel_path);
        // Fold a nested path down to the name of the entry directly inside
        // `dir` that the panel actually lists a row for.
        let Ok(rel_to_dir) = full.strip_prefix(&canon_dir) else { continue };
        let Some(first) = rel_to_dir.components().next() else { continue };
        let name = OsString::from(first.as_os_str());
        let Some(status) = classify(entry.status()) else { continue };
        result
            .entry(name)
            .and_modify(|existing: &mut FileStatus| {
                if status.rank() > existing.rank() {
                    *existing = status;
                }
            })
            .or_insert(status);
    }
    result
}

/// `rel` (a path below the repository workdir) spelled as a pathspec that
/// matches the directory literally rather than as a pattern (git-info
/// "Directory names are matched literally"; git-info-timeout-memory design
/// D4).
///
/// Components are joined with `/`, libgit2's separator (`git2` rewrites `\`
/// to `/` in pathspecs on Windows anyway, so no output of this function ever
/// contains a `\`). A path free of libgit2's wildcard characters (`[`, `*`,
/// `?`, plus `\` off Windows, where libgit2 reads it as an escape) is passed
/// through unchanged — the form that already handles `!old`, `#notes`,
/// spaces, and `]` today. Otherwise each wildcard character becomes a
/// one-character bracket class (`[[]`, `[*]`, `[?]`), a non-Windows `\`
/// becomes `?`, a leading `!` or `#` (which libgit2 would read as negation
/// or a comment) becomes `?`, and `/*` is appended: the directory-prefix
/// fallback is off for wildcard patterns, and since `*` also matches `/` the
/// suffix covers entries at any depth, including unexpanded untracked
/// subdirectories. A leading `?` can also match a sibling differing only in
/// the first character; those entries are removed by `pathspec_status`'s
/// `strip_prefix` fold.
pub fn literal_pathspec(rel: &Path) -> String {
    let joined = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
    let has_pattern_chars = joined.contains(['[', '*', '?']) || (!cfg!(windows) && joined.contains('\\'));
    if !has_pattern_chars {
        return joined;
    }
    let mut quoted = String::with_capacity(joined.len() + 2);
    for (i, ch) in joined.chars().enumerate() {
        match ch {
            '[' => quoted.push_str("[[]"),
            '*' => quoted.push_str("[*]"),
            '?' => quoted.push_str("[?]"),
            '\\' => quoted.push('?'),
            '!' | '#' if i == 0 => quoted.push('?'),
            _ => quoted.push(ch),
        }
    }
    quoted.push_str("/*");
    quoted
}

/// Map a raw libgit2 status bitflag set to the single marker this panel
/// shows, or `None` for a clean/ignored/current entry (git-info "Clean
/// entry has blank marker").
fn classify(status: Status) -> Option<FileStatus> {
    let working_tree_dirty = status.intersects(Status::WT_MODIFIED | Status::WT_DELETED | Status::WT_RENAMED | Status::WT_TYPECHANGE);
    let staged =
        status.intersects(Status::INDEX_NEW | Status::INDEX_MODIFIED | Status::INDEX_DELETED | Status::INDEX_RENAMED | Status::INDEX_TYPECHANGE);
    let untracked = status.contains(Status::WT_NEW);

    if working_tree_dirty {
        Some(FileStatus::Modified)
    } else if staged {
        Some(FileStatus::Staged)
    } else if untracked {
        Some(FileStatus::Untracked)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A fresh, empty temp directory for one test's fixture repo, cleaned up
    /// on drop.
    struct TempRepoDir(PathBuf);

    impl TempRepoDir {
        fn new(suffix: &str) -> TempRepoDir {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("filecommand-git-info-test-{}-{suffix}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create temp repo dir");
            TempRepoDir(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempRepoDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Initialize a repo at `dir` with a first commit on `main` (or
    /// whatever the local git2 default is) so `HEAD` resolves to a branch.
    fn init_repo_with_commit(dir: &Path) -> Repository {
        let repo = Repository::init(dir).expect("init repo");
        {
            let mut config = repo.config().expect("repo config");
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@example.com").unwrap();
        }
        std::fs::write(dir.join("committed.txt"), b"hello\n").unwrap();
        {
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("committed.txt")).unwrap();
            index.write().unwrap();
            let tree_id = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            let sig = repo.signature().unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[]).unwrap();
        }
        repo
    }

    #[test]
    fn directory_outside_any_repository_has_no_info() {
        let dir = TempRepoDir::new("outside");
        let info = query(dir.path());
        assert_eq!(info, GitInfo::none());
        assert!(!info.is_repo());
    }

    #[test]
    fn branch_resolves_for_a_directory_inside_a_repository() {
        let dir = TempRepoDir::new("branch");
        let repo = init_repo_with_commit(dir.path());
        let branch = resolve_branch(&repo).expect("HEAD should resolve to a branch after the first commit");
        let info = query(dir.path());
        assert_eq!(info.branch.as_deref(), Some(branch.as_str()));
        assert!(info.is_repo());
    }

    #[test]
    fn detection_works_from_a_subdirectory_of_the_repo() {
        let dir = TempRepoDir::new("subdir");
        init_repo_with_commit(dir.path());
        let sub = dir.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let info = query(&sub);
        assert!(info.is_repo(), "a subdirectory of a repo's working tree must still resolve");
    }

    #[test]
    fn modified_file_is_marked_m() {
        let dir = TempRepoDir::new("modified");
        init_repo_with_commit(dir.path());
        std::fs::write(dir.path().join("committed.txt"), b"changed\n").unwrap();
        let info = query(dir.path());
        assert_eq!(info.statuses.get(OsStr::new("committed.txt")), Some(&FileStatus::Modified));
    }

    #[test]
    fn untracked_and_staged_files_get_their_own_markers() {
        let dir = TempRepoDir::new("untracked-staged");
        let repo = init_repo_with_commit(dir.path());
        std::fs::write(dir.path().join("new_untracked.txt"), b"x\n").unwrap();
        std::fs::write(dir.path().join("new_staged.txt"), b"y\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("new_staged.txt")).unwrap();
        index.write().unwrap();

        let info = query(dir.path());
        assert_eq!(info.statuses.get(OsStr::new("new_untracked.txt")), Some(&FileStatus::Untracked));
        assert_eq!(info.statuses.get(OsStr::new("new_staged.txt")), Some(&FileStatus::Staged));
    }

    #[test]
    fn clean_file_has_no_status_entry() {
        let dir = TempRepoDir::new("clean");
        init_repo_with_commit(dir.path());
        let info = query(dir.path());
        assert_eq!(info.statuses.get(OsStr::new("committed.txt")), None, "an unmodified tracked file must render a blank marker");
    }

    #[test]
    fn status_is_scoped_to_the_panel_directory_via_pathspec() {
        let dir = TempRepoDir::new("pathspec-scope");
        let repo = init_repo_with_commit(dir.path());
        let sub_a = dir.path().join("a");
        let sub_b = dir.path().join("b");
        std::fs::create_dir_all(&sub_a).unwrap();
        std::fs::create_dir_all(&sub_b).unwrap();
        std::fs::write(sub_a.join("only_in_a.txt"), b"x\n").unwrap();
        std::fs::write(sub_b.join("only_in_b.txt"), b"y\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a/only_in_a.txt")).unwrap();
        index.add_path(Path::new("b/only_in_b.txt")).unwrap();
        index.write().unwrap();

        // Querying panel directory `a` must report `a`'s own entries but
        // nothing that lives under sibling directory `b`.
        let info_a = query(&sub_a);
        assert_eq!(info_a.statuses.get(OsStr::new("only_in_a.txt")), Some(&FileStatus::Staged));
        assert!(!info_a.statuses.contains_key(OsStr::new("only_in_b.txt")));
    }

    #[test]
    fn untracked_directory_is_marked_but_its_contents_are_not_enumerated() {
        let dir = TempRepoDir::new("untracked-dir");
        init_repo_with_commit(dir.path());
        let sub = dir.path().join("untracked_sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("inner1.txt"), b"1\n").unwrap();
        std::fs::write(sub.join("inner2.txt"), b"2\n").unwrap();

        let info = query(dir.path());
        // The subdirectory itself rolls up to a single `?` entry ...
        assert_eq!(info.statuses.get(OsStr::new("untracked_sub")), Some(&FileStatus::Untracked));
        // ... and libgit2 (with recurse_untracked_dirs off) never reported
        // its individual files, so no stray entries leak in under other
        // names.
        assert_eq!(info.statuses.len(), 1, "only the directory itself should be reported, not its contents");
    }

    #[test]
    fn stale_result_discarding_is_a_generation_key_concern_of_the_reducer() {
        // `query` itself is a pure, single-shot lookup with no notion of
        // "stale" or "timed out" — that guard lives in `update.rs` via
        // `PanelState::git_request`, exercised in `update::tests`. This
        // test simply documents the seam so the intent isn't lost: two
        // independent calls to `query` for the same directory are not
        // required to be reused or cached by this module.
        let dir = TempRepoDir::new("stale-doc");
        init_repo_with_commit(dir.path());
        let first = query(dir.path());
        let second = query(dir.path());
        assert_eq!(first, second);
    }

    // -----------------------------------------------------------------
    // git-info-timeout-memory: literal pathspecs, root discovery,
    // remembered-root membership (design D2, D4)
    // -----------------------------------------------------------------

    /// Stage `paths` (repo-relative, `/`-separated) and commit them on top
    /// of HEAD, so they can then be dirtied in the working tree.
    fn commit_paths(repo: &Repository, paths: &[&str], message: &str) {
        let mut index = repo.index().unwrap();
        for path in paths {
            index.add_path(Path::new(path)).unwrap();
        }
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent]).unwrap();
    }

    /// Extend the fixture with `<rel_dir>/a.txt` committed and then
    /// modified, plus an untracked `<rel_dir>/new/inner.txt`, so a panel
    /// sitting in `rel_dir` should report exactly `a.txt: M` and `new: ?` —
    /// the same markers any plain-named directory would show.
    fn commit_then_dirty(repo: &Repository, root: &Path, rel_dir: &str) {
        let dir = root.join(rel_dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"before\n").unwrap();
        let rel_file = format!("{rel_dir}/a.txt");
        commit_paths(repo, &[&rel_file], "add a.txt");
        std::fs::write(dir.join("a.txt"), b"after\n").unwrap();
        std::fs::create_dir_all(dir.join("new")).unwrap();
        std::fs::write(dir.join("new").join("inner.txt"), b"x\n").unwrap();
    }

    /// The panel directory reports its own modified `a.txt` and untracked
    /// `new`, and nothing else (git-info "Directory names are matched
    /// literally").
    fn assert_own_markers(info: &GitInfo) {
        assert_eq!(info.statuses.get(OsStr::new("a.txt")), Some(&FileStatus::Modified), "a.txt must show M: {:?}", info.statuses);
        assert_eq!(info.statuses.get(OsStr::new("new")), Some(&FileStatus::Untracked), "new must show ?: {:?}", info.statuses);
        assert_eq!(info.statuses.len(), 2, "nothing outside the panel directory is reported: {:?}", info.statuses);
    }

    #[test]
    fn literal_pathspec_passes_paths_without_pattern_characters_through_unchanged() {
        // The forms that already work today keep their exact spelling —
        // `!old` (would be negation), `#notes` (would be a comment),
        // spaces, and `]` included (design D4).
        assert_eq!(literal_pathspec(Path::new("src")), "src");
        assert_eq!(literal_pathspec(Path::new("src/main")), "src/main");
        assert_eq!(literal_pathspec(Path::new("!old")), "!old");
        assert_eq!(literal_pathspec(Path::new("#notes")), "#notes");
        assert_eq!(literal_pathspec(Path::new("a b")), "a b");
        assert_eq!(literal_pathspec(Path::new("a]b")), "a]b");
        assert_eq!(literal_pathspec(Path::new("x/!b")), "x/!b", "a `!` that is not the pattern's first character needs no quoting");
    }

    #[test]
    fn literal_pathspec_quotes_wildcard_characters_as_bracket_classes() {
        assert_eq!(literal_pathspec(Path::new("foo[1]")), "foo[[]1]/*");
        assert_eq!(literal_pathspec(Path::new("[x]")), "[[]x]/*");
        assert_eq!(literal_pathspec(Path::new("x/foo[1]")), "x/foo[[]1]/*");
        assert_eq!(literal_pathspec(Path::new("a*b")), "a[*]b/*");
        assert_eq!(literal_pathspec(Path::new("what?")), "what[?]/*");
    }

    #[test]
    fn literal_pathspec_defuses_a_leading_bang_or_hash_when_quoting() {
        // Inside a quoted pattern a leading `!` or `#` would still be read
        // as negation/comment, so it becomes `?`; a sibling the `?` also
        // matches is removed by the caller's `strip_prefix` fold (design D4).
        assert_eq!(literal_pathspec(Path::new("!b[2]")), "?b[[]2]/*");
        assert_eq!(literal_pathspec(Path::new("#c[3]")), "?c[[]3]/*");
        assert_eq!(literal_pathspec(Path::new("x/!b[2]")), "x/!b[[]2]/*", "only the pattern's first character is special");
    }

    #[cfg(not(windows))]
    #[test]
    fn literal_pathspec_replaces_a_backslash_off_windows() {
        // libgit2 reads `\` as an escape; a name containing one exists only
        // where the filesystem allows it.
        assert_eq!(literal_pathspec(Path::new("back\\slash")), "back?slash/*");
    }

    #[test]
    fn literal_pathspec_never_emits_a_backslash() {
        for rel in ["foo[1]", "[x]", "!b[2]", "#c[3]", "a*b", "what?", "x/foo[1]"] {
            assert!(!literal_pathspec(Path::new(rel)).contains('\\'), "{rel}");
        }
        #[cfg(not(windows))]
        assert!(!literal_pathspec(Path::new("back\\slash")).contains('\\'));
    }

    #[test]
    fn a_directory_named_foo_bracket_1_reports_its_markers() {
        // git-info "Directory names are matched literally".
        let dir = TempRepoDir::new("bracket");
        let repo = init_repo_with_commit(dir.path());
        commit_then_dirty(&repo, dir.path(), "foo[1]");
        let info = query(&dir.path().join("foo[1]"));
        assert_own_markers(&info);
    }

    #[test]
    fn pattern_characters_in_a_nested_directory_name() {
        // git-info "Pattern characters in a nested directory name": the
        // sibling `src/foo1` is walked too (the iterator prefix ends at the
        // first quoted character) but its entries never reach the panel.
        let dir = TempRepoDir::new("nested-bracket");
        let repo = init_repo_with_commit(dir.path());
        commit_then_dirty(&repo, dir.path(), "src/foo[1]");
        commit_then_dirty(&repo, dir.path(), "src/foo1");
        let info = query(&dir.path().join("src").join("foo[1]"));
        assert_own_markers(&info);
    }

    #[test]
    fn a_directory_named_bracket_x_reports_its_markers() {
        let dir = TempRepoDir::new("bracket-x");
        let repo = init_repo_with_commit(dir.path());
        commit_then_dirty(&repo, dir.path(), "[x]");
        let info = query(&dir.path().join("[x]"));
        assert_own_markers(&info);
    }

    #[test]
    fn a_directory_named_bang_b_bracket_2_reports_only_its_own_markers() {
        // The quoted pattern `?b[[]2]/*` also matches the sibling `Xb[2]`;
        // its entries are dropped by the `strip_prefix` fold (design D4).
        let dir = TempRepoDir::new("bang-bracket");
        let repo = init_repo_with_commit(dir.path());
        commit_then_dirty(&repo, dir.path(), "!b[2]");
        commit_then_dirty(&repo, dir.path(), "Xb[2]");
        let info = query(&dir.path().join("!b[2]"));
        assert_own_markers(&info);
    }

    #[test]
    fn a_directory_named_hash_c_bracket_3_reports_its_markers() {
        let dir = TempRepoDir::new("hash-bracket");
        let repo = init_repo_with_commit(dir.path());
        commit_then_dirty(&repo, dir.path(), "#c[3]");
        let info = query(&dir.path().join("#c[3]"));
        assert_own_markers(&info);
    }

    #[test]
    fn a_leading_exclamation_mark_is_not_a_negation() {
        // git-info "A leading exclamation mark is not a negation" — this
        // name passes through unchanged and keeps working via libgit2's
        // literal-`!` fallback; pinned so the quoting fix cannot regress it.
        let dir = TempRepoDir::new("bang");
        let repo = init_repo_with_commit(dir.path());
        commit_then_dirty(&repo, dir.path(), "!old");
        let info = query(&dir.path().join("!old"));
        assert_own_markers(&info);
    }

    #[test]
    fn a_leading_hash_does_not_widen_the_query() {
        // git-info "A leading hash does not widen the query": the
        // comment-pattern keeps the walk scoped via the iterator bound, so
        // a modified file outside `#notes` is not reported either.
        let dir = TempRepoDir::new("hash");
        let repo = init_repo_with_commit(dir.path());
        commit_then_dirty(&repo, dir.path(), "#notes");
        std::fs::write(dir.path().join("outside.txt"), b"before\n").unwrap();
        commit_paths(&repo, &["outside.txt"], "add outside.txt");
        std::fs::write(dir.path().join("outside.txt"), b"after\n").unwrap();
        let info = query(&dir.path().join("#notes"));
        assert_own_markers(&info);
        assert!(!info.statuses.contains_key(OsStr::new("outside.txt")), "outside the panel directory: {:?}", info.statuses);
    }

    #[test]
    fn a_directory_name_with_a_space_reports_its_markers() {
        let dir = TempRepoDir::new("space");
        let repo = init_repo_with_commit(dir.path());
        commit_then_dirty(&repo, dir.path(), "my dir");
        let info = query(&dir.path().join("my dir"));
        assert_own_markers(&info);
    }

    #[cfg(not(windows))]
    #[test]
    fn directories_named_with_glob_characters_report_their_markers() {
        // `*`, `?`, and `\` cannot appear in a Windows file name, so these
        // run only where the filesystem allows them.
        for (i, name) in ["a*b", "what?", "back\\slash"].iter().enumerate() {
            let dir = TempRepoDir::new(&format!("glob-{i}"));
            let repo = init_repo_with_commit(dir.path());
            commit_then_dirty(&repo, dir.path(), name);
            let info = query(&dir.path().join(name));
            assert_own_markers(&info);
        }
    }

    #[test]
    fn root_in_dir_spelling_walks_up_by_components() {
        assert_eq!(root_in_dir_spelling(Path::new("/repo/sub/deep"), 2), Some(PathBuf::from("/repo")));
        assert_eq!(root_in_dir_spelling(Path::new("/repo/sub"), 1), Some(PathBuf::from("/repo")));
    }

    #[test]
    fn root_in_dir_spelling_of_the_root_itself_is_the_directory() {
        assert_eq!(root_in_dir_spelling(Path::new("/repo"), 0), Some(PathBuf::from("/repo")));
    }

    #[cfg(windows)]
    #[test]
    fn root_in_dir_spelling_preserves_the_queried_paths_own_spelling() {
        // The panel may hold the 8.3 short-name form while libgit2 reports
        // the long one; the root must come back in the panel's spelling so
        // later `cwd`s compare equal under `is_under_root` (design D2).
        assert_eq!(root_in_dir_spelling(Path::new(r"C:\VERYLO~1\sub"), 1), Some(PathBuf::from(r"C:\VERYLO~1")));
    }

    #[test]
    fn discover_reports_the_depth_so_the_root_spells_like_the_query() {
        let dir = TempRepoDir::new("discover-depth");
        init_repo_with_commit(dir.path());
        let sub = dir.path().join("a").join("b");
        std::fs::create_dir_all(&sub).unwrap();
        let (_repo, depth) = discover(&sub).expect("a subdirectory of a repo's working tree is discovered");
        assert_eq!(depth, 2);
        assert_eq!(root_in_dir_spelling(&sub, depth).as_deref(), Some(dir.path()));
    }

    #[test]
    fn discover_outside_any_repository_is_none() {
        let dir = TempRepoDir::new("discover-outside");
        assert!(discover(dir.path()).is_none());
    }

    #[test]
    fn is_under_root_covers_the_root_itself_and_its_descendants() {
        assert!(is_under_root(Path::new("/mono"), Path::new("/mono")));
        assert!(is_under_root(Path::new("/mono/src/deep"), Path::new("/mono")));
        assert!(!is_under_root(Path::new("/other"), Path::new("/mono")));
        assert!(!is_under_root(Path::new("/mono"), Path::new("/mono/src")), "a shorter path is not under a longer root");
    }

    #[test]
    fn is_under_root_is_component_wise_not_a_string_prefix() {
        assert!(!is_under_root(Path::new("/mono2"), Path::new("/mono")));
        #[cfg(windows)]
        assert!(!is_under_root(Path::new(r"D:\mono2"), Path::new(r"D:\mono")));
    }

    #[cfg(windows)]
    #[test]
    fn is_under_root_folds_ascii_case_on_windows() {
        assert!(is_under_root(Path::new(r"d:\MONO\Src"), Path::new(r"D:\mono")));
    }
}
