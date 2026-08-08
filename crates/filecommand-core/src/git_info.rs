//! Background git repository detection: enclosing-repo lookup, branch
//! resolution, and pathspec-scoped per-entry status via `git2` (libgit2).
//!
//! [`query`] is the whole blocking operation as one call — repo discovery,
//! branch resolution, and status — designed to be run on a dedicated worker
//! thread that `filecommand-tui` owns (core never spawns threads itself, and
//! libgit2 status calls are blocking and not cancellable mid-call; design
//! D3). The reducer (`update.rs`) guards every reply with a directory +
//! generation key (`PanelState::git_request`) via `Command::GitInfoResolved`,
//! so a result for a directory/generation the panel has since moved past is
//! dropped rather than applied — the same staleness-guard shape as
//! `PanelState::info_request` (git-info "Silent absence on timeout and
//! stale-result discarding").

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::Path;

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
/// repository detection").
pub fn query(dir: &Path) -> GitInfo {
    let Ok(repo) = Repository::discover(dir) else {
        return GitInfo::none();
    };
    let branch = resolve_branch(&repo);
    let statuses = pathspec_status(&repo, dir);
    GitInfo { branch, statuses }
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
    let Ok(rel_dir) = dir.strip_prefix(workdir) else {
        return result;
    };

    let mut opts = StatusOptions::new();
    opts.include_untracked(true);
    // Report an untracked directory as a single entry rather than walking
    // into it (git-info "Untracked directory not expanded").
    opts.recurse_untracked_dirs(false);
    if !rel_dir.as_os_str().is_empty() {
        let pattern = rel_dir.to_string_lossy().replace('\\', "/");
        opts.pathspec(pattern);
    }

    let Ok(statuses) = repo.statuses(Some(&mut opts)) else {
        return result;
    };
    for entry in statuses.iter() {
        let Some(rel_path) = entry.path() else { continue };
        let full = workdir.join(rel_path);
        // Fold a nested path down to the name of the entry directly inside
        // `dir` that the panel actually lists a row for.
        let Ok(rel_to_dir) = full.strip_prefix(dir) else { continue };
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

/// Map a raw libgit2 status bitflag set to the single marker this panel
/// shows, or `None` for a clean/ignored/current entry (git-info "Clean
/// entry has blank marker").
fn classify(status: Status) -> Option<FileStatus> {
    let working_tree_dirty =
        status.intersects(Status::WT_MODIFIED | Status::WT_DELETED | Status::WT_RENAMED | Status::WT_TYPECHANGE);
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
}
