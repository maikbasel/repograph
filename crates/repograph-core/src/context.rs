//! Agent-facing context aggregation.
//!
//! The `Context` envelope is the payload `repograph context` emits — for each
//! in-scope repository, the inlined content of every file matching the user's
//! selected agents' file patterns. Stable JSON shape is the contract with
//! downstream AI agents.
//!
//! Pattern resolution is deliberately bounded: flat patterns (e.g. `CLAUDE.md`)
//! are checked by direct existence, glob patterns under a known parent
//! directory (e.g. `.cursor/rules/*.md`) are matched against a single non-
//! recursive directory listing. We never walk the repo tree blindly — that's
//! the kind of behavior that would force us to add `.gitignore` semantics to
//! avoid traversing `node_modules`, and the registry's patterns don't need it.

use std::path::{Path, PathBuf};

use globset::{Glob, GlobSetBuilder};
use serde::{Serialize, Serializer};

use crate::agents::AgentId;

/// One agent's matched files within a single repo.
#[derive(Debug, Clone, Serialize)]
pub struct AgentDoc {
    pub agent: AgentId,
    pub files: Vec<MatchedFile>,
}

/// One file the resolver matched against an agent's pattern set.
///
/// `path` is the file's path relative to the repo root, normalized to use
/// forward slashes regardless of the host platform (downstream agents resolve
/// relative paths against the repo, and absolute paths leak local filesystem
/// layout).
#[derive(Debug, Clone)]
pub struct MatchedFile {
    pub path: PathBuf,
    pub bytes: u64,
    pub content: String,
}

impl Serialize for MatchedFile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("MatchedFile", 3)?;
        let path_str = self
            .path
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        state.serialize_field("path", &path_str)?;
        state.serialize_field("bytes", &self.bytes)?;
        state.serialize_field("content", &self.content)?;
        state.end()
    }
}

/// Per-repo block of the `Context` envelope.
#[derive(Debug, Clone, Serialize)]
pub struct RepoContext {
    pub name: String,
    pub path: PathBuf,
    pub branch: Option<String>,
    pub agent_docs: Vec<AgentDoc>,
    pub warnings: Vec<String>,
}

/// The scope a `repograph context` invocation was resolved against.
///
/// Echoed back into the payload so downstream consumers can identify which
/// slice of the registry they're seeing without parsing back from the repos
/// array.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scope {
    All,
    Workspace { name: String },
    Repos { repos: Vec<String> },
}

/// Top-level payload emitted by `repograph context`. `schema_version` is the
/// contract — additive-only at `1`; breaking changes bump it.
#[derive(Debug, Clone, Serialize)]
pub struct Context {
    pub schema_version: u32,
    pub generated_at: String,
    pub agents: Vec<AgentId>,
    pub scope: Scope,
    pub repos: Vec<RepoContext>,
    pub warnings: Vec<String>,
}

/// The current (and only) `Context` schema version. Downstream agents pin on
/// this; bumping it is a breaking change.
pub const SCHEMA_VERSION: u32 = 1;

impl RepoContext {
    /// Build a `RepoContext` for a single repository. Failures are inline:
    /// missing paths, unreadable directories, and per-file read failures
    /// surface as warning strings rather than aborting the batch.
    ///
    /// `branch` is `None` for missing / unborn / detached / bare repos. The
    /// missing case also writes a top-level warning so callers don't have to
    /// re-derive the missing state from `branch.is_none()`.
    #[must_use]
    pub fn build_one(name: &str, repo_path: &Path, agents: &[AgentId]) -> Self {
        let mut warnings: Vec<String> = Vec::new();

        // Canonicalize early. A missing path produces a placeholder entry
        // with no agent docs and a single warning — consistent with how the
        // git-status spec handles repos that drifted away under the user.
        let canonical = match fs_err::canonicalize(repo_path) {
            Ok(p) => p,
            Err(e) => {
                return Self {
                    name: name.to_string(),
                    path: repo_path.to_path_buf(),
                    branch: None,
                    agent_docs: Vec::new(),
                    warnings: vec![format!("path no longer accessible: {e}")],
                };
            }
        };

        let (branch, branch_warning) = read_branch(&canonical);
        if let Some(w) = branch_warning {
            warnings.push(w);
        }

        let (agent_docs, doc_warnings) = resolve_agent_docs(&canonical, agents);
        warnings.extend(doc_warnings);

        Self {
            name: name.to_string(),
            path: canonical,
            branch,
            agent_docs,
            warnings,
        }
    }
}

/// Read the current branch name from a repo. Returns `(None, None)` for
/// healthy repos in detached / unborn / bare state (these are not errors —
/// the context payload just has `branch: null`). Returns `(None, Some(msg))`
/// when the path is no longer a git repo at all, which is a warning-worthy
/// drift.
fn read_branch(repo_path: &Path) -> (Option<String>, Option<String>) {
    let repo = match git2::Repository::open(repo_path) {
        Ok(r) => r,
        Err(e) => {
            return (
                None,
                Some(format!(
                    "no longer a git repository at {}: {}",
                    repo_path.display(),
                    e.message()
                )),
            );
        }
    };

    if repo.is_bare() {
        return (None, None);
    }

    match repo.head() {
        Ok(head) if head.is_branch() => (head.shorthand().map(ToString::to_string), None),
        Ok(_) => (None, None), // detached
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => (None, None),
        Err(e) => (
            None,
            Some(format!("could not read HEAD: {}", e.message())),
        ),
    }
}

/// Walk the registry's patterns for each agent against `repo_root`.
///
/// Returns the per-agent `AgentDoc` blocks plus a flat list of warning
/// strings for files we couldn't read or files that weren't valid UTF-8.
///
/// Pattern handling:
///
/// - **Flat patterns** (no glob metacharacters, e.g. `CLAUDE.md`,
///   `.github/copilot-instructions.md`) are checked by direct existence
///   against `repo_root.join(pattern)`.
/// - **Glob patterns under a known parent directory** (e.g.
///   `.cursor/rules/*.md`) are matched against the entries of that parent
///   directory only — no recursion, no walking past the known prefix.
///
/// Files are deduplicated within a single agent's `files` by relative path
/// and sorted ascending for stable output.
#[must_use]
pub fn resolve_agent_docs(
    repo_root: &Path,
    agents: &[AgentId],
) -> (Vec<AgentDoc>, Vec<String>) {
    let mut docs = Vec::with_capacity(agents.len());
    let mut warnings = Vec::new();

    for agent in agents {
        let mut files = Vec::new();
        let mut seen: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();

        for pattern in agent.file_patterns() {
            match classify_pattern(pattern) {
                PatternKind::Flat(relpath) => {
                    let abs = repo_root.join(&relpath);
                    if let Some(matched) = read_matched_file(&abs, &relpath, &mut warnings) {
                        if seen.insert(relpath.clone()) {
                            files.push(matched);
                        }
                    }
                }
                PatternKind::Glob { parent, pattern } => {
                    expand_glob(
                        repo_root,
                        &parent,
                        &pattern,
                        &mut files,
                        &mut seen,
                        &mut warnings,
                    );
                }
            }
        }

        files.sort_by(|a, b| a.path.cmp(&b.path));
        docs.push(AgentDoc {
            agent: *agent,
            files,
        });
    }

    (docs, warnings)
}

enum PatternKind {
    /// A direct-file pattern with no glob metacharacters. Carries the
    /// relative path (parsed from the on-the-wire string, normalized for
    /// the host platform).
    Flat(PathBuf),
    /// A glob pattern whose parent directory is known and fixed. Carries the
    /// parent path (relative to the repo root) and the glob to match against
    /// the parent's direct children.
    Glob {
        parent: PathBuf,
        pattern: String,
    },
}

fn classify_pattern(pattern: &'static str) -> PatternKind {
    let has_glob = pattern.contains(['*', '?', '[']);
    if !has_glob {
        return PatternKind::Flat(PathBuf::from(pattern));
    }
    // Split into parent dir (literal prefix) and the trailing globbed segment.
    // Every v1 pattern has a single globbed final segment; we don't support
    // glob metacharacters in interior segments (no `.cursor/*/foo.md`).
    let (parent, leaf) = pattern.rsplit_once('/').map_or_else(
        || (PathBuf::from("."), pattern.to_string()),
        |(p, l)| (PathBuf::from(p), l.to_string()),
    );
    PatternKind::Glob {
        parent,
        pattern: leaf,
    }
}

fn expand_glob(
    repo_root: &Path,
    parent_rel: &Path,
    pattern: &str,
    files: &mut Vec<MatchedFile>,
    seen: &mut std::collections::BTreeSet<PathBuf>,
    warnings: &mut Vec<String>,
) {
    let parent_abs = repo_root.join(parent_rel);
    let entries = match fs_err::read_dir(&parent_abs) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            warnings.push(format!(
                "could not list {}: {e}",
                display_rel(parent_rel)
            ));
            return;
        }
    };

    let glob = match Glob::new(pattern) {
        Ok(g) => g,
        Err(e) => {
            warnings.push(format!("invalid glob '{pattern}': {e}"));
            return;
        }
    };
    let mut builder = GlobSetBuilder::new();
    builder.add(glob);
    let set = match builder.build() {
        Ok(s) => s,
        Err(e) => {
            warnings.push(format!("could not compile glob '{pattern}': {e}"));
            return;
        }
    };

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            continue;
        }
        let file_name = entry.file_name();
        if !set.is_match(&file_name) {
            continue;
        }
        let rel = parent_rel.join(&file_name);
        let abs = entry.path();
        if let Some(matched) = read_matched_file(&abs, &rel, warnings) {
            if seen.insert(rel) {
                files.push(matched);
            }
        }
    }
}

fn read_matched_file(
    abs: &Path,
    relpath: &Path,
    warnings: &mut Vec<String>,
) -> Option<MatchedFile> {
    let bytes = match fs_err::read(abs) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            warnings.push(format!("{}: {e}", display_rel(relpath)));
            return None;
        }
    };
    let len = bytes.len() as u64;
    let Ok(content) = String::from_utf8(bytes) else {
        warnings.push(format!(
            "{}: file is not valid UTF-8, skipped",
            display_rel(relpath)
        ));
        return None;
    };
    Some(MatchedFile {
        path: relpath.to_path_buf(),
        bytes: len,
        content,
    })
}

fn display_rel(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use tempfile::TempDir;

    fn init_repo(parent: &Path, name: &str) -> PathBuf {
        let path = parent.join(name);
        std::fs::create_dir_all(&path).unwrap();
        {
            let repo = git2::Repository::init(&path).unwrap();
            let sig = git2::Signature::now("T", "t@e").unwrap();
            let tree_id = {
                let mut index = repo.index().unwrap();
                index.write_tree().unwrap()
            };
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                .unwrap();
        }
        std::fs::canonicalize(&path).unwrap()
    }

    // ─── classify_pattern: every v1 registry pattern lands the right way ───

    #[test]
    fn classify_every_v1_pattern() {
        for agent in AgentId::all() {
            for pattern in agent.file_patterns() {
                let kind = classify_pattern(pattern);
                let has_meta = pattern.contains(['*', '?', '[']);
                match (kind, has_meta) {
                    (PatternKind::Flat(_), false) | (PatternKind::Glob { .. }, true) => {}
                    (PatternKind::Flat(_), true) => {
                        panic!("pattern {pattern:?} has glob meta but classified Flat");
                    }
                    (PatternKind::Glob { .. }, false) => {
                        panic!("pattern {pattern:?} has no glob meta but classified Glob");
                    }
                }
            }
        }
    }

    #[test]
    fn classify_flat_pattern_keeps_subdir_prefix() {
        match classify_pattern(".github/copilot-instructions.md") {
            PatternKind::Flat(p) => {
                assert_eq!(p, PathBuf::from(".github/copilot-instructions.md"));
            }
            PatternKind::Glob { .. } => panic!("expected Flat"),
        }
    }

    #[test]
    fn classify_glob_pattern_splits_parent_and_leaf() {
        match classify_pattern(".cursor/rules/*.md") {
            PatternKind::Glob { parent, pattern } => {
                assert_eq!(parent, PathBuf::from(".cursor/rules"));
                assert_eq!(pattern, "*.md");
            }
            PatternKind::Flat(_) => panic!("expected Glob"),
        }
    }

    // ─── resolve_agent_docs: file resolution end-to-end ────────────────────

    #[test]
    fn empty_repo_yields_empty_files_per_agent() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(tmp.path(), "r");
        let (docs, warnings) = resolve_agent_docs(&repo, &[AgentId::ClaudeCode, AgentId::Cursor]);
        assert_eq!(docs.len(), 2);
        assert!(docs[0].files.is_empty());
        assert!(docs[1].files.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn flat_pattern_resolves_to_root_level_file() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(tmp.path(), "r");
        std::fs::write(repo.join("CLAUDE.md"), "hello\n").unwrap();
        let (docs, warnings) = resolve_agent_docs(&repo, &[AgentId::ClaudeCode]);
        assert!(warnings.is_empty());
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].files.len(), 1);
        assert_eq!(docs[0].files[0].path, PathBuf::from("CLAUDE.md"));
        assert_eq!(docs[0].files[0].content, "hello\n");
        assert_eq!(docs[0].files[0].bytes, 6);
    }

    #[test]
    fn glob_pattern_expands_to_multiple_files_under_known_dir() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(tmp.path(), "r");
        std::fs::create_dir_all(repo.join(".cursor/rules")).unwrap();
        std::fs::write(repo.join(".cursor/rules/style.md"), "s").unwrap();
        std::fs::write(repo.join(".cursor/rules/tests.md"), "t").unwrap();

        let (docs, warnings) = resolve_agent_docs(&repo, &[AgentId::Cursor]);
        assert!(warnings.is_empty());
        assert_eq!(docs.len(), 1);
        let paths: Vec<_> = docs[0].files.iter().map(|f| f.path.clone()).collect();
        assert_eq!(
            paths,
            vec![
                PathBuf::from(".cursor/rules/style.md"),
                PathBuf::from(".cursor/rules/tests.md"),
            ],
            "files sorted by relative path"
        );
    }

    #[test]
    fn mixed_flat_and_glob_under_same_agent() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(tmp.path(), "r");
        std::fs::create_dir_all(repo.join(".cursor/rules")).unwrap();
        std::fs::write(repo.join(".cursor/rules/style.md"), "s").unwrap();
        std::fs::write(repo.join(".cursorrules"), "legacy").unwrap();

        let (docs, _) = resolve_agent_docs(&repo, &[AgentId::Cursor]);
        let paths: Vec<_> = docs[0].files.iter().map(|f| f.path.clone()).collect();
        assert_eq!(
            paths,
            vec![
                PathBuf::from(".cursor/rules/style.md"),
                PathBuf::from(".cursorrules"),
            ]
        );
    }

    #[test]
    fn nested_claude_md_does_not_match() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(tmp.path(), "r");
        std::fs::create_dir_all(repo.join("docs")).unwrap();
        std::fs::write(repo.join("docs/CLAUDE.md"), "nested").unwrap();

        let (docs, warnings) = resolve_agent_docs(&repo, &[AgentId::ClaudeCode]);
        assert!(docs[0].files.is_empty(), "no recursive walk");
        assert!(warnings.is_empty(), "missing root file is not a warning");
    }

    #[test]
    fn deep_node_modules_is_not_walked() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(tmp.path(), "r");
        // Plant a CLAUDE.md deep in node_modules. The resolver must not
        // surface it (and must not stall walking the tree to find it).
        let deep = repo.join("node_modules/some-pkg/internals/CLAUDE.md");
        std::fs::create_dir_all(deep.parent().unwrap()).unwrap();
        std::fs::write(&deep, "noise").unwrap();

        let (docs, _) = resolve_agent_docs(&repo, &[AgentId::ClaudeCode]);
        assert!(docs[0].files.is_empty());
    }

    #[test]
    fn non_utf8_file_is_skipped_with_warning() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(tmp.path(), "r");
        std::fs::write(repo.join(".cursorrules"), [0xFF, 0xFE]).unwrap();

        let (docs, warnings) = resolve_agent_docs(&repo, &[AgentId::Cursor]);
        assert!(docs[0].files.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains(".cursorrules") && warnings[0].contains("UTF-8"),
            "warning names file and reason, got: {warnings:?}"
        );
    }

    #[test]
    fn copilot_pattern_resolves_under_dot_github() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(tmp.path(), "r");
        std::fs::create_dir_all(repo.join(".github")).unwrap();
        std::fs::write(repo.join(".github/copilot-instructions.md"), "x").unwrap();

        let (docs, warnings) = resolve_agent_docs(&repo, &[AgentId::Copilot]);
        assert!(warnings.is_empty());
        assert_eq!(docs[0].files.len(), 1);
        assert_eq!(
            docs[0].files[0].path,
            PathBuf::from(".github/copilot-instructions.md")
        );
    }

    // ─── RepoContext::build_one: per-repo aggregator ───────────────────────

    #[test]
    fn build_one_populates_branch_and_files() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(tmp.path(), "r");
        std::fs::write(repo.join("CLAUDE.md"), "ctx\n").unwrap();

        let rc = RepoContext::build_one("r", &repo, &[AgentId::ClaudeCode]);
        assert_eq!(rc.name, "r");
        assert_eq!(rc.path, repo);
        assert!(rc.branch.is_some(), "branch resolved from HEAD");
        assert_eq!(rc.agent_docs.len(), 1);
        assert_eq!(rc.agent_docs[0].files.len(), 1);
        assert!(rc.warnings.is_empty());
    }

    #[test]
    fn build_one_missing_path_produces_placeholder_with_warning() {
        let tmp = TempDir::new().unwrap();
        let ghost = tmp.path().join("ghost");

        let rc = RepoContext::build_one("ghost", &ghost, &[AgentId::ClaudeCode]);
        assert_eq!(rc.branch, None);
        assert!(rc.agent_docs.is_empty());
        assert_eq!(rc.warnings.len(), 1);
        assert!(
            rc.warnings[0].to_lowercase().contains("accessible")
                || rc.warnings[0].to_lowercase().contains("no such")
        );
    }

    #[test]
    fn build_one_detached_head_yields_null_branch_no_warning() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(tmp.path(), "r");
        let git_repo = git2::Repository::open(&repo).unwrap();
        let head_id = {
            let head = git_repo.head().unwrap();
            let commit = head.peel_to_commit().unwrap();
            commit.id()
        };
        git_repo.set_head_detached(head_id).unwrap();
        drop(git_repo);

        let rc = RepoContext::build_one("r", &repo, &[]);
        assert_eq!(rc.branch, None);
        assert!(rc.warnings.is_empty(), "detached is not a warning state");
    }

    #[test]
    fn build_one_preserves_agent_order_from_input() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(tmp.path(), "r");
        std::fs::write(repo.join("CLAUDE.md"), "c").unwrap();
        std::fs::write(repo.join("AGENTS.md"), "a").unwrap();

        let rc = RepoContext::build_one(
            "r",
            &repo,
            &[AgentId::AgentsMd, AgentId::ClaudeCode],
        );
        assert_eq!(rc.agent_docs[0].agent, AgentId::AgentsMd);
        assert_eq!(rc.agent_docs[1].agent, AgentId::ClaudeCode);
    }

    // ─── Serialization shape ───────────────────────────────────────────────

    #[test]
    fn scope_serializes_with_tagged_kind() {
        let s = serde_json::to_value(Scope::All).unwrap();
        assert_eq!(s, serde_json::json!({ "kind": "all" }));

        let s = serde_json::to_value(Scope::Workspace {
            name: "team".into(),
        })
        .unwrap();
        assert_eq!(s, serde_json::json!({ "kind": "workspace", "name": "team" }));

        let s = serde_json::to_value(Scope::Repos {
            repos: vec!["a".into(), "b".into()],
        })
        .unwrap();
        assert_eq!(s, serde_json::json!({ "kind": "repos", "repos": ["a", "b"] }));
    }

    #[test]
    fn matched_file_path_serializes_with_forward_slashes() {
        let mf = MatchedFile {
            path: PathBuf::from(".cursor").join("rules").join("a.md"),
            bytes: 1,
            content: "x".into(),
        };
        let v = serde_json::to_value(&mf).unwrap();
        assert_eq!(v["path"], ".cursor/rules/a.md");
        assert_eq!(v["bytes"], 1);
        assert_eq!(v["content"], "x");
    }

    #[test]
    fn full_context_envelope_serializes_with_documented_keys() {
        let context = Context {
            schema_version: SCHEMA_VERSION,
            generated_at: "2026-05-24T00:00:00Z".into(),
            agents: vec![AgentId::ClaudeCode],
            scope: Scope::All,
            repos: vec![RepoContext {
                name: "r".into(),
                path: PathBuf::from("/tmp/r"),
                branch: Some("main".into()),
                agent_docs: vec![AgentDoc {
                    agent: AgentId::ClaudeCode,
                    files: vec![MatchedFile {
                        path: PathBuf::from("CLAUDE.md"),
                        bytes: 5,
                        content: "hello".into(),
                    }],
                }],
                warnings: vec![],
            }],
            warnings: vec![],
        };
        let v = serde_json::to_value(&context).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["generated_at"], "2026-05-24T00:00:00Z");
        assert_eq!(v["agents"][0], "claude-code");
        assert_eq!(v["scope"]["kind"], "all");
        assert_eq!(v["repos"][0]["name"], "r");
        assert_eq!(v["repos"][0]["branch"], "main");
        assert_eq!(v["repos"][0]["agent_docs"][0]["agent"], "claude-code");
        assert_eq!(
            v["repos"][0]["agent_docs"][0]["files"][0]["path"],
            "CLAUDE.md"
        );
        assert!(v["warnings"].is_array() && v["warnings"].as_array().unwrap().is_empty());
    }
}
