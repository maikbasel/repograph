//! Shared helpers for repograph acceptance tests.
//!
//! `mod.rs` (rather than `common.rs`) prevents Cargo from compiling this as a
//! standalone test binary; other tests pull it in via `mod common;`.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use git2::{BranchType, IndexAddOption, Repository, Signature};

/// Build a `repograph` command with `REPOGRAPH_CONFIG_DIR` pointed at `config_dir`.
/// Strips any inherited `REPOGRAPH_CONFIG_DIR` first so the test always controls it.
pub fn repograph_cmd(config_dir: &Path) -> Command {
    assert!(
        config_dir.is_absolute(),
        "test bug: config_dir must be absolute, got {}",
        config_dir.display()
    );
    let mut cmd = Command::cargo_bin("repograph").expect("repograph binary built");
    cmd.env_remove("REPOGRAPH_CONFIG_DIR")
        .env("REPOGRAPH_CONFIG_DIR", config_dir);
    cmd
}

/// Build a `repograph` command that uses the `--config-dir <path>` flag instead
/// of the env var. Strips any inherited env var so the flag is the only signal.
pub fn repograph_cmd_with_flag(config_dir: &Path) -> Command {
    assert!(
        config_dir.is_absolute(),
        "test bug: config_dir must be absolute, got {}",
        config_dir.display()
    );
    let mut cmd = Command::cargo_bin("repograph").expect("repograph binary built");
    cmd.env_remove("REPOGRAPH_CONFIG_DIR")
        .arg("--config-dir")
        .arg(config_dir);
    cmd
}

/// Build a command with no config-dir hint at all (no flag, env var stripped).
/// Used by the platform-no-default scenario.
pub fn repograph_cmd_no_config_hint() -> Command {
    let mut cmd = Command::cargo_bin("repograph").expect("repograph binary built");
    cmd.env_remove("REPOGRAPH_CONFIG_DIR");
    cmd
}

/// Initialize a real git repository at `parent.join(name)` with one commit so
/// HEAD exists. Returns the canonicalized absolute path.
pub fn fixture_git_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).expect("create fixture dir");
    let repo = Repository::init(&path).expect("git init");
    let sig = Signature::now("Test", "test@example.com").expect("signature");
    let tree_id = {
        let mut index = repo.index().expect("repo index");
        index.write_tree().expect("write empty tree")
    };
    {
        let tree = repo.find_tree(tree_id).expect("find empty tree");
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .expect("initial commit");
    }
    drop(repo);
    std::fs::canonicalize(&path).expect("canonicalize fixture path")
}

/// Parse JSON output from `repograph list --json` and return the `repos` array.
pub fn parse_repos_json(stdout: &[u8]) -> Vec<serde_json::Value> {
    let parsed: serde_json::Value = serde_json::from_slice(stdout).expect("stdout is valid JSON");
    parsed
        .get("repos")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .expect("envelope contains a `repos` array")
}

/// Parse the full JSON envelope from `repograph list --json`.
pub fn parse_list_json(stdout: &[u8]) -> serde_json::Value {
    serde_json::from_slice(stdout).expect("stdout is valid JSON")
}

/// Parse JSON output from `repograph workspace ls --json` and return the
/// `workspaces` array.
pub fn parse_workspaces_json(stdout: &[u8]) -> Vec<serde_json::Value> {
    let parsed: serde_json::Value = serde_json::from_slice(stdout).expect("stdout is valid JSON");
    parsed
        .get("workspaces")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .expect("envelope contains a `workspaces` array")
}

/// Parse the full JSON envelope from `repograph workspace show --json`.
pub fn parse_workspace_show_json(stdout: &[u8]) -> serde_json::Value {
    serde_json::from_slice(stdout).expect("stdout is valid JSON")
}

/// Parse JSON output from `repograph status --json` and return the `repos`
/// array.
pub fn parse_status_json(stdout: &[u8]) -> Vec<serde_json::Value> {
    let parsed: serde_json::Value = serde_json::from_slice(stdout).expect("stdout is valid JSON");
    parsed
        .get("repos")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .expect("envelope contains a `repos` array")
}

/// Initialize a bare git repository at `parent.join(name)`. Returns the
/// canonicalized absolute path.
pub fn fixture_bare_git_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).expect("create fixture dir");
    Repository::init_bare(&path).expect("git init --bare");
    std::fs::canonicalize(&path).expect("canonicalize fixture path")
}

/// Initialize an unborn git repository (no commits, HEAD unborn) at
/// `parent.join(name)`. Returns the canonicalized absolute path.
pub fn fixture_unborn_git_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).expect("create fixture dir");
    Repository::init(&path).expect("git init");
    std::fs::canonicalize(&path).expect("canonicalize fixture path")
}

/// Initialize a git repo with one commit, then modify a tracked file so the
/// working tree is dirty (one unstaged change). Returns the canonicalized path.
pub fn fixture_dirty_git_repo(parent: &Path, name: &str) -> PathBuf {
    let path = fixture_git_repo(parent, name);
    let repo = Repository::open(&path).expect("open dirty fixture");
    let file = path.join("tracked.txt");
    std::fs::write(&file, "initial\n").expect("write tracked file");
    let sig = Signature::now("Test", "test@example.com").expect("signature");
    let tree_id = {
        let mut index = repo.index().expect("index");
        index
            .add_all(["tracked.txt"], IndexAddOption::DEFAULT, None)
            .expect("add file");
        index.write().expect("write index");
        index.write_tree().expect("write tree")
    };
    {
        let tree = repo.find_tree(tree_id).expect("find tree");
        let parent_commit = repo
            .head()
            .expect("HEAD")
            .peel_to_commit()
            .expect("parent commit");
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "track file",
            &tree,
            &[&parent_commit],
        )
        .expect("commit tracked");
    }
    drop(repo);
    // Modify the tracked file so the working tree is now dirty.
    std::fs::write(&file, "modified\n").expect("modify tracked file");
    path
}

/// Initialize a git repo with one commit and detach HEAD onto it. Returns the
/// canonicalized path.
pub fn fixture_detached_git_repo(parent: &Path, name: &str) -> PathBuf {
    let path = fixture_git_repo(parent, name);
    let repo = Repository::open(&path).expect("open detached fixture");
    let head_id = {
        let head = repo.head().expect("HEAD").peel_to_commit().expect("commit");
        head.id()
    };
    repo.set_head_detached(head_id).expect("detach HEAD");
    drop(repo);
    path
}

/// Set up an upstream tracking branch for `repo_path`'s current branch by
/// cloning into a bare sibling repo, registering it as `origin`, fetching, and
/// configuring `branch.<name>.remote`/`merge`. Returns the canonical upstream
/// bare-repo path.
pub fn attach_upstream(repo_path: &Path, branch: &str) -> PathBuf {
    let parent = repo_path.parent().expect("repo has parent");
    let bare_dir = parent.join(format!(
        "{}.upstream",
        repo_path.file_name().unwrap().to_string_lossy()
    ));
    Repository::init_bare(&bare_dir).expect("init upstream bare");
    {
        let local = Repository::open(repo_path).expect("open local for upstream");
        let bare_url = bare_dir.to_string_lossy().to_string();
        local
            .remote("origin", &bare_url)
            .expect("set origin remote");
        // Push current branch to bare so the upstream ref exists.
        {
            let mut origin = local.find_remote("origin").expect("origin");
            let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
            origin.push(&[&refspec], None).expect("push to upstream");
        }
        // Configure tracking.
        {
            let mut config = local.config().expect("local config");
            config
                .set_str(&format!("branch.{branch}.remote"), "origin")
                .expect("branch.remote");
            config
                .set_str(
                    &format!("branch.{branch}.merge"),
                    &format!("refs/heads/{branch}"),
                )
                .expect("branch.merge");
        }
        // Run a fetch so origin/<branch> ref is materialized locally.
        {
            let mut origin = local.find_remote("origin").expect("origin (fetch)");
            origin
                .fetch(&[branch], None, None)
                .expect("initial fetch from upstream");
        }
    }
    std::fs::canonicalize(&bare_dir).expect("canonicalize bare path")
}

/// Append `count` empty commits to the local branch of the repo at `repo_path`.
pub fn add_local_commits(repo_path: &Path, count: usize) {
    let repo = Repository::open(repo_path).expect("open repo");
    let sig = Signature::now("Test", "test@example.com").expect("signature");
    for i in 0..count {
        let parent_commit = repo.head().expect("HEAD").peel_to_commit().expect("parent");
        let tree = parent_commit.tree().expect("parent tree");
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            &format!("local commit {i}"),
            &tree,
            &[&parent_commit],
        )
        .expect("local commit");
    }
}

/// Append `count` empty commits to the `branch` ref of the bare upstream at
/// `bare_path`, then run `git fetch` on the local repo so `origin/<branch>` is
/// up to date. Used to manufacture a "behind" state without a real fetch step
/// in the unit under test.
pub fn add_upstream_commits(local_repo: &Path, upstream_bare: &Path, branch: &str, count: usize) {
    // Easiest path: create a working clone of the bare, commit there, push,
    // then fetch on the local.
    let parent = upstream_bare.parent().expect("bare has parent");
    let work = parent.join("upstream-work");
    if work.exists() {
        std::fs::remove_dir_all(&work).expect("clean upstream work");
    }
    let cloned = Repository::clone(&upstream_bare.to_string_lossy(), &work)
        .expect("clone bare for upstream commits");
    let sig = Signature::now("Test", "test@example.com").expect("signature");
    for i in 0..count {
        let parent_commit = cloned
            .head()
            .expect("HEAD")
            .peel_to_commit()
            .expect("parent");
        let tree = parent_commit.tree().expect("parent tree");
        cloned
            .commit(
                Some("HEAD"),
                &sig,
                &sig,
                &format!("upstream commit {i}"),
                &tree,
                &[&parent_commit],
            )
            .expect("upstream commit");
    }
    let mut origin = cloned.find_remote("origin").expect("clone origin");
    origin
        .push(&[&format!("refs/heads/{branch}:refs/heads/{branch}")], None)
        .expect("push upstream commits back to bare");
    drop(origin);
    drop(cloned);
    std::fs::remove_dir_all(&work).expect("clean upstream work after push");
    // Fetch on the local so origin/<branch> moves forward without altering local HEAD.
    let local = Repository::open(local_repo).expect("open local for fetch");
    let mut origin = local.find_remote("origin").expect("origin on local");
    origin
        .fetch(&[branch], None, None)
        .expect("fetch new upstream commits");
    drop(origin);
    // Sanity: confirm the upstream branch exists in local refs.
    let _ = local
        .find_branch(&format!("origin/{branch}"), BranchType::Remote)
        .expect("remote tracking branch present");
}

/// Stage a new file (without committing) so the working tree has one staged
/// change. Returns nothing; the repo is mutated in place.
pub fn stage_new_file(repo_path: &Path, name: &str) {
    let repo = Repository::open(repo_path).expect("open repo for staging");
    std::fs::write(repo_path.join(name), "staged\n").expect("write staged file");
    let mut index = repo.index().expect("index");
    index
        .add_all([name], IndexAddOption::DEFAULT, None)
        .expect("add staged");
    index.write().expect("write index");
}

/// Drop an untracked file (no `git add`) so the working tree has one untracked
/// entry.
pub fn add_untracked_file(repo_path: &Path, name: &str) {
    std::fs::write(repo_path.join(name), "untracked\n").expect("write untracked");
}
