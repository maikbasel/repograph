//! `git2`-backed introspection helpers.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::RepographError;

/// Verify that `path` is a git repository, returning its canonical absolute
/// form on success. Symlinks are resolved; relative inputs are absolutized.
///
/// # Errors
///
/// Returns [`RepographError::NotFound`] when `path` does not exist on disk, or
/// [`RepographError::GitOpen`] when it exists but is not a git repository.
pub fn validate_git_repo(path: &Path) -> Result<PathBuf, RepographError> {
    let canonical = match crate::path::canonicalize(path) {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(RepographError::NotFound {
                kind: "path",
                name: path.display().to_string(),
            });
        }
        Err(e) => return Err(e.into()),
    };

    git2::Repository::open(&canonical).map_err(|source| RepographError::GitOpen {
        path: canonical.clone(),
        source,
    })?;

    Ok(canonical)
}

/// Coarse classification of a registered repository's runtime state. Drives
/// the `state` column in TTY output and the `state` field in the JSON
/// envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RepoState {
    /// Working tree clean; HEAD on a branch.
    Clean,
    /// Working tree has staged, unstaged, or untracked changes.
    Dirty,
    /// HEAD does not point at a branch.
    Detached,
    /// Repository initialized but no commits exist yet.
    Unborn,
    /// Bare repository — no working tree.
    Bare,
    /// Registered path no longer resolves to an accessible git repository.
    Missing,
}

/// Per-repository status snapshot produced by [`inspect`]. Field order is the
/// documented JSON serialization order.
#[derive(Debug, Clone, Serialize)]
pub struct RepoStatus {
    pub name: String,
    pub path: PathBuf,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    pub state: RepoState,
    pub error: Option<String>,
    /// Short SHA exposed for the CLI's detached-HEAD `warn!` line. Skipped
    /// from JSON output: the agent contract is `branch: null` + `state:
    /// detached`, the SHA is a diagnostic for humans.
    #[serde(skip)]
    pub detached_sha: Option<String>,
}

impl RepoStatus {
    fn missing(name: &str, path: &Path, error: String) -> Self {
        Self::stub(name, path, RepoState::Missing, Some(error))
    }

    fn bare(name: &str, path: &Path) -> Self {
        Self::stub(name, path, RepoState::Bare, Some("bare repository".into()))
    }

    fn stub(name: &str, path: &Path, state: RepoState, error: Option<String>) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_path_buf(),
            branch: None,
            upstream: None,
            ahead: 0,
            behind: 0,
            dirty: false,
            staged: 0,
            unstaged: 0,
            untracked: 0,
            state,
            error,
            detached_sha: None,
        }
    }
}

/// Inspect a single registered repository and return its `RepoStatus`.
///
/// This function does not propagate errors via `Result`. Per the git-status
/// design, the failure surface for an individual repo is per-row — a missing
/// path, a broken `.git` directory, or a failed fetch all become a populated
/// `error` field on the returned status, never an aborted batch.
///
/// When `fetch == true` and the repo is in `Clean`/`Dirty` state with a
/// resolvable upstream, `git2::Remote::fetch` is invoked against the upstream
/// remote of the current branch before `ahead`/`behind` are computed. Fetch
/// failures populate `error` and leave ahead/behind reflecting the pre-fetch
/// state.
#[must_use]
pub fn inspect(name: &str, path: &Path, fetch: bool) -> RepoStatus {
    // Canonicalize so the row's `path` matches what's stored in the registry
    // post-validation. If the path no longer exists or isn't accessible,
    // surface as missing without a stack trace.
    let canonical = match crate::path::canonicalize(path) {
        Ok(p) => p,
        Err(e) => {
            return RepoStatus::missing(name, path, format!("{e}"));
        }
    };

    let repo = match git2::Repository::open(&canonical) {
        Ok(r) => r,
        Err(e) => {
            return RepoStatus::missing(name, &canonical, format!("{e}"));
        }
    };

    if repo.is_bare() {
        return RepoStatus::bare(name, &canonical);
    }

    // HEAD state: unborn (no commit), detached (HEAD points at a commit, no
    // branch shorthand), or on a branch.
    let head = repo.head();
    let head_err = match head {
        Ok(h) => Ok(h),
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => Err(HeadFlavor::Unborn),
        Err(e) => {
            return RepoStatus::missing(name, &canonical, format!("{e}"));
        }
    };

    let mut status = RepoStatus {
        name: name.to_string(),
        path: canonical,
        branch: None,
        upstream: None,
        ahead: 0,
        behind: 0,
        dirty: false,
        staged: 0,
        unstaged: 0,
        untracked: 0,
        state: RepoState::Clean,
        error: None,
        detached_sha: None,
    };

    let (staged, unstaged, untracked) = count_statuses(&repo);
    status.staged = staged;
    status.unstaged = unstaged;
    status.untracked = untracked;
    status.dirty = staged + unstaged + untracked > 0;

    match head_err {
        Err(HeadFlavor::Unborn) => {
            status.state = RepoState::Unborn;
            return status;
        }
        Ok(head) => {
            if head.is_branch() {
                let branch_name = head.shorthand().map(ToString::to_string);
                status.branch.clone_from(&branch_name);
                status.state = if status.dirty {
                    RepoState::Dirty
                } else {
                    RepoState::Clean
                };

                if let Some(branch) = branch_name.as_deref() {
                    if let Some(upstream_ref) = upstream_full_ref(&repo, branch) {
                        status.upstream = upstream_short(&repo, &upstream_ref);

                        if fetch {
                            if let Err(fetch_err) = run_fetch(&repo, branch) {
                                status.error = Some(fetch_err);
                            }
                        }

                        if let Some((ahead, behind)) =
                            compute_ahead_behind(&repo, &head, &upstream_ref)
                        {
                            status.ahead = u32::try_from(ahead).unwrap_or(u32::MAX);
                            status.behind = u32::try_from(behind).unwrap_or(u32::MAX);
                        }
                    }
                }
            } else {
                // Detached HEAD.
                status.state = RepoState::Detached;
                if let Ok(commit) = head.peel_to_commit() {
                    let oid = commit.id();
                    status.detached_sha = Some(short_oid(&oid));
                }
            }
        }
    }

    status
}

enum HeadFlavor {
    Unborn,
}

/// Walk `git2::Statuses` and count entries in three categories:
/// staged (index ↔ HEAD diff), unstaged (worktree ↔ index diff), untracked.
/// `.gitignored` entries are excluded.
fn count_statuses(repo: &git2::Repository) -> (u32, u32, u32) {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true)
        .include_ignored(false)
        .exclude_submodules(false)
        .recurse_untracked_dirs(true);
    let Ok(statuses) = repo.statuses(Some(&mut opts)) else {
        return (0, 0, 0);
    };
    let mut staged = 0u32;
    let mut unstaged = 0u32;
    let mut untracked = 0u32;
    for entry in statuses.iter() {
        let (s, u, t) = classify(entry.status());
        if s {
            staged = staged.saturating_add(1);
        }
        if u {
            unstaged = unstaged.saturating_add(1);
        }
        if t {
            untracked = untracked.saturating_add(1);
        }
    }
    (staged, unstaged, untracked)
}

/// Map a `git2::Status` bitflag to `(staged, unstaged, untracked)` booleans.
/// A single entry can be staged-and-unstaged simultaneously (e.g. modified
/// after being staged); both bits are reported.
const fn classify(status: git2::Status) -> (bool, bool, bool) {
    let staged = status.intersects(git2::Status::from_bits_truncate(
        git2::Status::INDEX_NEW.bits()
            | git2::Status::INDEX_MODIFIED.bits()
            | git2::Status::INDEX_DELETED.bits()
            | git2::Status::INDEX_RENAMED.bits()
            | git2::Status::INDEX_TYPECHANGE.bits(),
    ));
    let unstaged = status.intersects(git2::Status::from_bits_truncate(
        git2::Status::WT_MODIFIED.bits()
            | git2::Status::WT_DELETED.bits()
            | git2::Status::WT_RENAMED.bits()
            | git2::Status::WT_TYPECHANGE.bits(),
    ));
    let untracked = status.contains(git2::Status::WT_NEW) && !staged;
    (staged, unstaged, untracked)
}

fn upstream_full_ref(repo: &git2::Repository, branch: &str) -> Option<String> {
    let local_ref = format!("refs/heads/{branch}");
    let upstream_buf = repo.branch_upstream_name(&local_ref).ok()?;
    upstream_buf.as_str().map(ToString::to_string)
}

fn upstream_short(repo: &git2::Repository, full_ref: &str) -> Option<String> {
    let reference = repo.find_reference(full_ref).ok()?;
    reference.shorthand().map(ToString::to_string)
}

fn compute_ahead_behind(
    repo: &git2::Repository,
    head: &git2::Reference<'_>,
    upstream_full_ref: &str,
) -> Option<(usize, usize)> {
    let local_oid = head.target()?;
    let upstream_ref = repo.find_reference(upstream_full_ref).ok()?;
    let upstream_oid = upstream_ref.target()?;
    repo.graph_ahead_behind(local_oid, upstream_oid).ok()
}

fn run_fetch(repo: &git2::Repository, branch: &str) -> Result<(), String> {
    // Resolve the remote name from `branch.<name>.remote`. Fall back to
    // "origin" if that fails — matches `git fetch` behavior.
    let config = repo.config().map_err(|e| e.message().to_string())?;
    let remote_name = config
        .get_string(&format!("branch.{branch}.remote"))
        .unwrap_or_else(|_| "origin".to_string());
    let mut remote = repo
        .find_remote(&remote_name)
        .map_err(|e| e.message().to_string())?;

    let mut callbacks = git2::RemoteCallbacks::new();
    // libgit2 invokes this callback repeatedly with different `allowed_types`
    // as it tries successive auth methods. We track each branch with a flag so
    // a single fetch doesn't loop indefinitely against the same failing method.
    let mut tried_ssh_agent = false;
    let mut tried_cred_helper = false;
    let mut tried_default = false;
    callbacks.credentials(move |url, username_from_url, allowed_types| {
        if allowed_types.contains(git2::CredentialType::SSH_KEY) && !tried_ssh_agent {
            tried_ssh_agent = true;
            let user = username_from_url.unwrap_or("git");
            return git2::Cred::ssh_key_from_agent(user);
        }
        if allowed_types.contains(git2::CredentialType::USER_PASS_PLAINTEXT) && !tried_cred_helper {
            tried_cred_helper = true;
            // Open default git config (~/.gitconfig + system) so the user's
            // credential.helper setting (Keychain, libsecret, manager-core,
            // etc.) is honored — matches what `git fetch` does on the shell.
            let cfg = git2::Config::open_default()?;
            return git2::Cred::credential_helper(&cfg, url, username_from_url);
        }
        if allowed_types.contains(git2::CredentialType::DEFAULT) && !tried_default {
            tried_default = true;
            return git2::Cred::default();
        }
        Err(git2::Error::from_str(
            "no usable credential available (ssh-agent / credential helper exhausted)",
        ))
    });

    let mut fo = git2::FetchOptions::new();
    fo.remote_callbacks(callbacks);
    remote
        .fetch(&[branch], Some(&mut fo), None)
        .map_err(|e| e.message().to_string())?;
    Ok(())
}

fn short_oid(oid: &git2::Oid) -> String {
    let s = oid.to_string();
    s.chars().take(7).collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn rejects_nonexistent_path() {
        let tmp = TempDir::new().unwrap();
        let err = validate_git_repo(&tmp.path().join("nope")).unwrap_err();
        assert!(matches!(err, RepographError::NotFound { kind: "path", .. }));
    }

    #[test]
    fn rejects_non_git_directory() {
        let tmp = TempDir::new().unwrap();
        let err = validate_git_repo(tmp.path()).unwrap_err();
        assert!(matches!(err, RepographError::GitOpen { .. }));
    }

    #[test]
    fn accepts_real_git_repo_returns_canonical() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("r");
        std::fs::create_dir_all(&repo_path).unwrap();
        git2::Repository::init(&repo_path).unwrap();

        let resolved = validate_git_repo(&repo_path).unwrap();
        assert_eq!(resolved, crate::path::canonicalize(&repo_path).unwrap());
    }

    // ─── inspect() unit tests ──────────────────────────────────────────────

    /// Init a repo with one empty commit. Returns the repo dir.
    fn init_with_commit(dir: &Path) {
        let repo = git2::Repository::init(dir).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = {
            let mut index = repo.index().unwrap();
            index.write_tree().unwrap()
        };
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
    }

    #[test]
    fn inspect_clean_repo_no_upstream_is_clean() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("r");
        std::fs::create_dir_all(&dir).unwrap();
        init_with_commit(&dir);
        let s = inspect("r", &dir, false);
        assert_eq!(s.state, RepoState::Clean);
        assert!(s.error.is_none());
        assert!(s.branch.is_some());
        assert!(s.upstream.is_none());
        assert!(!s.dirty);
    }

    #[test]
    fn inspect_dirty_repo_reports_dirty_with_unstaged() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("r");
        std::fs::create_dir_all(&dir).unwrap();
        init_with_commit(&dir);
        // Add a tracked file via second commit, then modify it.
        let repo = git2::Repository::open(&dir).unwrap();
        let file = dir.join("tracked.txt");
        std::fs::write(&file, "hello\n").unwrap();
        {
            let mut index = repo.index().unwrap();
            index
                .add_all(["tracked.txt"], git2::IndexAddOption::DEFAULT, None)
                .unwrap();
            index.write().unwrap();
            let tree_id = index.write_tree().unwrap();
            let sig = git2::Signature::now("T", "t@e").unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            let parent = repo.head().unwrap().peel_to_commit().unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "track", &tree, &[&parent])
                .unwrap();
        }
        drop(repo);
        std::fs::write(&file, "modified\n").unwrap();

        let s = inspect("r", &dir, false);
        assert_eq!(s.state, RepoState::Dirty);
        assert!(s.dirty);
        assert!(s.unstaged >= 1);
    }

    #[test]
    fn inspect_untracked_file_alone_reports_dirty() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("r");
        std::fs::create_dir_all(&dir).unwrap();
        init_with_commit(&dir);
        std::fs::write(dir.join("new.txt"), "x").unwrap();

        let s = inspect("r", &dir, false);
        assert_eq!(s.state, RepoState::Dirty);
        assert_eq!(s.untracked, 1);
    }

    #[test]
    fn inspect_staged_only_reports_dirty() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("r");
        std::fs::create_dir_all(&dir).unwrap();
        init_with_commit(&dir);
        let repo = git2::Repository::open(&dir).unwrap();
        std::fs::write(dir.join("staged.txt"), "x").unwrap();
        {
            let mut index = repo.index().unwrap();
            index
                .add_all(["staged.txt"], git2::IndexAddOption::DEFAULT, None)
                .unwrap();
            index.write().unwrap();
        }
        drop(repo);

        let s = inspect("r", &dir, false);
        assert_eq!(s.staged, 1);
        assert_eq!(s.untracked, 0);
        assert_eq!(s.state, RepoState::Dirty);
    }

    #[test]
    fn inspect_detached_head_reports_detached() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("r");
        std::fs::create_dir_all(&dir).unwrap();
        init_with_commit(&dir);
        let repo = git2::Repository::open(&dir).unwrap();
        let head_id = {
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            head.id()
        };
        repo.set_head_detached(head_id).unwrap();
        drop(repo);

        let s = inspect("r", &dir, false);
        assert_eq!(s.state, RepoState::Detached);
        assert!(s.branch.is_none());
        assert!(s.detached_sha.as_deref().map_or(0, str::len) == 7);
    }

    #[test]
    fn inspect_unborn_repo_reports_unborn() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("r");
        std::fs::create_dir_all(&dir).unwrap();
        git2::Repository::init(&dir).unwrap();

        let s = inspect("r", &dir, false);
        assert_eq!(s.state, RepoState::Unborn);
        assert!(s.branch.is_none());
        assert!(s.error.is_none());
    }

    #[test]
    fn inspect_bare_repo_reports_bare() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("r.git");
        std::fs::create_dir_all(&dir).unwrap();
        git2::Repository::init_bare(&dir).unwrap();

        let s = inspect("r", &dir, false);
        assert_eq!(s.state, RepoState::Bare);
        assert!(s.error.is_some());
    }

    #[test]
    fn inspect_missing_path_reports_missing() {
        let tmp = TempDir::new().unwrap();
        let s = inspect("r", &tmp.path().join("gone"), false);
        assert_eq!(s.state, RepoState::Missing);
        assert!(s.error.is_some());
    }

    #[test]
    fn inspect_directory_without_git_dir_reports_missing() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("r");
        std::fs::create_dir_all(&dir).unwrap();
        let s = inspect("r", &dir, false);
        assert_eq!(s.state, RepoState::Missing);
        assert!(s.error.is_some());
    }

    #[test]
    fn classify_index_new_is_staged() {
        let (staged, unstaged, untracked) = classify(git2::Status::INDEX_NEW);
        assert!(staged);
        assert!(!unstaged);
        assert!(!untracked);
    }

    #[test]
    fn classify_worktree_new_is_untracked() {
        let (staged, unstaged, untracked) = classify(git2::Status::WT_NEW);
        assert!(!staged);
        assert!(!unstaged);
        assert!(untracked);
    }

    #[test]
    fn classify_index_new_plus_worktree_modified_is_staged_and_unstaged() {
        let combined = git2::Status::INDEX_NEW | git2::Status::WT_MODIFIED;
        let (staged, unstaged, untracked) = classify(combined);
        assert!(staged);
        assert!(unstaged);
        assert!(!untracked);
    }
}
