//! Cross-repo precedent search: index the git-tracked content of every
//! registered repo, then retrieve it by meaning or by keyword.
//!
//! The store ([`index`]) is a single `SQLite` database spanning all repos so one
//! query reaches everything — the answer to "I solved this somewhere, I just
//! don't remember which repo." Retrieval is hybrid: BM25 lexical (FTS5) fused
//! with semantic cosine over local embeddings ([`embed`], feature `semantic`),
//! merged by reciprocal-rank fusion. The binary resolves the data directory and
//! passes it in; this module performs the `dirs`-free path joins, mirroring how
//! `config.rs` takes a `dir: &Path`.

pub mod chunk;
pub mod embed;
pub mod index;

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::RepographError;
use crate::search::index::{Embedder, Store, fuse};

/// File name of the central index database under the data directory.
pub const INDEX_DB_NAME: &str = "index.db";

/// Subdirectory of the data directory that caches the embedding model.
pub const MODEL_SUBDIR: &str = "models";

/// Schema version of the `repograph find` JSON envelope. Additive-only at `1`.
pub const FIND_SCHEMA_VERSION: u32 = 1;

/// Candidate-pool multiplier: we pull `limit * POOL_FACTOR` candidates from each
/// retrieval arm before fusing, so fusion has room to reorder.
const POOL_FACTOR: usize = 5;

/// Floor on the candidate pool, so small `--limit` values still fuse usefully.
const MIN_POOL: usize = 50;

/// Maximum characters in a result snippet before truncation.
const SNIPPET_MAX_CHARS: usize = 400;

/// One ranked search result. Field order is the JSON serialization order and
/// part of the stable output contract.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub repo: String,
    /// Repo-relative path, forward-slashed.
    pub path: String,
    /// 1-based start line of the matched chunk.
    pub line: u32,
    /// Fused relevance score (higher is better).
    pub score: f64,
    pub snippet: String,
}

/// Outcome of [`search`]: the ranked hits plus retrieval diagnostics for the
/// binary's stderr (never part of the stdout data contract).
#[derive(Debug, Clone)]
pub struct SearchOutcome {
    pub hits: Vec<Hit>,
    /// True when semantic retrieval actually contributed (embedder available
    /// and vectors present).
    pub semantic_used: bool,
    /// Set when semantic was requested but unavailable — the reason, for a
    /// stderr notice. `None` when not requested or fully satisfied.
    pub degraded: Option<String>,
}

/// Outcome of [`build_index`].
#[derive(Debug, Clone, Default)]
pub struct IndexOutcome {
    pub repos_indexed: usize,
    pub repos_skipped: usize,
    pub files_indexed: usize,
    pub files_unchanged: usize,
    pub files_purged: usize,
    /// True when at least one file was (re)indexed or purged this run.
    pub changed: bool,
    /// True when semantic embeddings were written.
    pub semantic: bool,
    /// Set when semantic was requested but unavailable.
    pub degraded: Option<String>,
}

/// Health of the search index, consumed by `repograph doctor`.
#[derive(Debug, Clone, Default)]
pub struct IndexStatus {
    /// The index database file exists.
    pub present: bool,
    /// The index opened and matched this build's schema.
    pub readable: bool,
    /// Repos that are missing from the index or stale relative to their HEAD.
    pub stale: Vec<String>,
}

/// Path to the index database within `data_dir`.
#[must_use]
pub fn index_db_path(data_dir: &Path) -> PathBuf {
    data_dir.join(INDEX_DB_NAME)
}

/// Path to the embedding-model cache within `data_dir`.
#[must_use]
pub fn model_cache_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(MODEL_SUBDIR)
}

/// Build or refresh the index over `repos` (each `(name, absolute_path)`).
///
/// Indexing is incremental and git-aware: only changed files are re-chunked,
/// removed files are purged. Repos that cannot be opened, are bare, or have no
/// commits are skipped with a warning rather than aborting the run.
///
/// # Errors
///
/// Returns [`RepographError::Index`] on a store failure, or
/// [`RepographError::Io`] if the data directory cannot be created.
pub fn build_index(
    data_dir: &Path,
    repos: &[(String, PathBuf)],
    semantic: bool,
) -> Result<IndexOutcome, RepographError> {
    let mut store = Store::open_for_build(&index_db_path(data_dir))?;
    let (mut embedder, degraded) = make_embedder(semantic, &model_cache_dir(data_dir));
    if let Some(e) = embedder.as_ref() {
        store.ensure_model(e.model_id())?;
    }

    let mut outcome = IndexOutcome {
        semantic: embedder.is_some(),
        degraded,
        ..IndexOutcome::default()
    };

    for (name, path) in repos {
        let repo = match git2::Repository::open(path) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(repo = %name, error = %e, "skipping repo: cannot open");
                outcome.repos_skipped += 1;
                continue;
            }
        };
        if repo.is_bare() {
            tracing::warn!(repo = %name, "skipping bare repo");
            outcome.repos_skipped += 1;
            continue;
        }
        let files = match chunk::tracked_files(&repo, path) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(repo = %name, error = %e, "skipping repo: cannot read index");
                outcome.repos_skipped += 1;
                continue;
            }
        };
        let head = head_commit(&repo);
        #[allow(clippy::option_if_let_else)]
        let emb: Option<&mut dyn Embedder> = match &mut embedder {
            Some(e) => Some(e.as_mut()),
            None => None,
        };
        let stats = store.reconcile_repo(name, &files, head.as_deref(), emb)?;
        outcome.repos_indexed += 1;
        outcome.files_indexed += stats.files_indexed;
        outcome.files_unchanged += stats.files_unchanged;
        outcome.files_purged += stats.files_purged;
    }
    outcome.changed = outcome.files_indexed > 0 || outcome.files_purged > 0;
    Ok(outcome)
}

/// Search the index, returning ranked hits across all repos or one workspace.
///
/// `repos_filter` (when non-empty) scopes results to those repo names.
/// `semantic` requests the hybrid path; it degrades to lexical with a populated
/// `degraded` reason when embeddings or the model are unavailable.
///
/// # Errors
///
/// Returns [`RepographError::IndexMissing`] (exit 3) when no index has been
/// built, or [`RepographError::Index`] (exit 1) when the index is unreadable.
pub fn search(
    data_dir: &Path,
    query: &str,
    repos_filter: &[String],
    limit: usize,
    semantic: bool,
) -> Result<SearchOutcome, RepographError> {
    let store = Store::open_existing(&index_db_path(data_dir))?;
    let pool = limit.max(1).saturating_mul(POOL_FACTOR).max(MIN_POOL);

    let lexical = store.search_lexical(query, repos_filter, pool)?;

    let mut vector = Vec::new();
    let mut semantic_used = false;
    let mut degraded = None;

    if semantic {
        let (embedder, deg) = make_embedder(true, &model_cache_dir(data_dir));
        degraded = deg;
        if let Some(mut e) = embedder {
            if store.has_vectors()? {
                match e.embed(&[query.to_string()]) {
                    Ok(v) if !v.is_empty() => {
                        vector = store.search_vectors(&v[0], repos_filter, pool)?;
                        semantic_used = true;
                    }
                    Ok(_) => degraded = Some("query produced no embedding".to_string()),
                    Err(msg) => degraded = Some(msg),
                }
            } else {
                degraded =
                    Some("index has no embeddings — run `repograph index --semantic`".to_string());
            }
        }
    }

    let fused = fuse(&[lexical.as_slice(), vector.as_slice()]);
    let top: Vec<i64> = fused.iter().take(limit).map(|(id, _)| *id).collect();
    let rows = store.fetch_chunks(&top)?;
    let hits = fused
        .iter()
        .take(limit)
        .filter_map(|(id, score)| {
            rows.get(id).map(|row| Hit {
                repo: row.repo.clone(),
                path: row.path.clone(),
                line: row.start_line,
                score: *score,
                snippet: snippet(&row.content),
            })
        })
        .collect();

    Ok(SearchOutcome {
        hits,
        semantic_used,
        degraded,
    })
}

/// Compute the [`IndexStatus`] for `repos`.
///
/// Never errors on a missing or unreadable index — those are reported via the
/// `present`/`readable` flags so `doctor` can surface them as warnings without
/// aborting.
///
/// # Errors
///
/// Returns [`RepographError::Index`] only if the (readable) index fails a query
/// mid-inspection.
pub fn index_health(
    data_dir: &Path,
    repos: &[(String, PathBuf)],
) -> Result<IndexStatus, RepographError> {
    let db = index_db_path(data_dir);
    if !db.is_file() {
        return Ok(IndexStatus::default());
    }
    let store = match Store::open_existing(&db) {
        Ok(s) => s,
        Err(RepographError::IndexMissing) => return Ok(IndexStatus::default()),
        Err(_) => {
            return Ok(IndexStatus {
                present: true,
                readable: false,
                stale: Vec::new(),
            });
        }
    };
    let commits = store.indexed_commits()?;
    let mut stale = Vec::new();
    for (name, path) in repos {
        let current = git2::Repository::open(path)
            .ok()
            .and_then(|r| head_commit(&r));
        match commits.get(name) {
            Some(indexed) if *indexed == current => {}
            _ => stale.push(name.clone()),
        }
    }
    stale.sort();
    Ok(IndexStatus {
        present: true,
        readable: true,
        stale,
    })
}

/// Construct an embedder when `semantic` is requested. Returns `(None,
/// Some(reason))` when semantic is requested but unavailable, `(Some, None)` on
/// success, and `(None, None)` when semantic was not requested.
fn make_embedder(
    semantic: bool,
    model_cache_dir: &Path,
) -> (Option<Box<dyn Embedder>>, Option<String>) {
    if !semantic {
        return (None, None);
    }
    match embed::create(model_cache_dir) {
        Ok(e) => (Some(e), None),
        Err(reason) => (None, Some(reason)),
    }
}

fn head_commit(repo: &git2::Repository) -> Option<String> {
    repo.head().ok()?.target().map(|oid| oid.to_string())
}

/// Trim a chunk's content to a bounded snippet, appending an ellipsis when cut.
fn snippet(content: &str) -> String {
    if content.chars().count() <= SNIPPET_MAX_CHARS {
        return content.to_string();
    }
    let truncated: String = content.chars().take(SNIPPET_MAX_CHARS).collect();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::format_collect)]
    use super::*;
    use tempfile::TempDir;

    fn init_repo(parent: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = parent.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = git2::Repository::init(&dir).unwrap();
        for (rel, body) in files {
            std::fs::write(dir.join(rel), body).unwrap();
        }
        let sig = git2::Signature::now("T", "t@e").unwrap();
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        dir
    }

    #[test]
    fn build_then_search_across_repos() {
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("data");
        let api = init_repo(
            tmp.path(),
            "api",
            &[("auth.rs", "fn rotate_refresh_token() {}\n")],
        );
        let ui = init_repo(
            tmp.path(),
            "ui",
            &[("button.rs", "fn render_button() {}\n")],
        );
        let repos = vec![("api".to_string(), api), ("ui".to_string(), ui)];

        let outcome = build_index(&data, &repos, false).unwrap();
        assert_eq!(outcome.repos_indexed, 2);
        assert!(outcome.files_indexed >= 2);

        let result = search(&data, "rotate_refresh_token", &[], 5, false).unwrap();
        assert!(!result.hits.is_empty());
        assert_eq!(result.hits[0].repo, "api");
        assert_eq!(result.hits[0].path, "auth.rs");
        assert!(!result.semantic_used);
    }

    #[test]
    fn search_without_index_is_index_missing() {
        let tmp = TempDir::new().unwrap();
        let err = search(&tmp.path().join("data"), "anything", &[], 5, false).unwrap_err();
        assert!(matches!(err, RepographError::IndexMissing));
    }

    #[test]
    fn workspace_filter_scopes_results() {
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("data");
        let api = init_repo(tmp.path(), "api", &[("a.rs", "fn shared_widget() {}\n")]);
        let ui = init_repo(tmp.path(), "ui", &[("b.rs", "fn shared_widget() {}\n")]);
        let repos = vec![("api".to_string(), api), ("ui".to_string(), ui)];
        build_index(&data, &repos, false).unwrap();

        let scoped = search(&data, "shared_widget", &["api".to_string()], 5, false).unwrap();
        assert!(!scoped.hits.is_empty());
        assert!(scoped.hits.iter().all(|h| h.repo == "api"));
    }

    #[test]
    fn no_match_is_empty_not_error() {
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("data");
        let api = init_repo(tmp.path(), "api", &[("a.rs", "fn alpha() {}\n")]);
        build_index(&data, &[("api".to_string(), api)], false).unwrap();
        let result = search(&data, "zzz_nonexistent_symbol_qqq", &[], 5, false).unwrap();
        assert!(result.hits.is_empty());
    }

    #[test]
    fn limit_bounds_hits() {
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("data");
        let body: String = (0..50).map(|n| format!("fn widget_{n}() {{}}\n")).collect();
        let api = init_repo(tmp.path(), "api", &[("w.rs", &body)]);
        build_index(&data, &[("api".to_string(), api)], false).unwrap();
        let result = search(&data, "widget", &[], 3, false).unwrap();
        assert!(result.hits.len() <= 3);
    }

    #[test]
    fn semantic_requested_without_feature_degrades_to_lexical() {
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("data");
        let api = init_repo(tmp.path(), "api", &[("a.rs", "fn parse_csv() {}\n")]);
        build_index(&data, &[("api".to_string(), api)], true).unwrap();
        let result = search(&data, "parse_csv", &[], 5, true).unwrap();
        // Lexical still returns the hit; semantic did not contribute.
        assert!(!result.hits.is_empty());
        if cfg!(not(feature = "semantic")) {
            assert!(!result.semantic_used);
            assert!(result.degraded.is_some());
        }
    }

    #[test]
    fn health_missing_index_is_absent_not_error() {
        let tmp = TempDir::new().unwrap();
        let status = index_health(&tmp.path().join("data"), &[]).unwrap();
        assert!(!status.present);
        assert!(status.stale.is_empty());
    }

    #[test]
    fn health_reports_current_and_stale() {
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("data");
        let api = init_repo(tmp.path(), "api", &[("a.rs", "fn a() {}\n")]);
        let repos = vec![("api".to_string(), api.clone())];
        build_index(&data, &repos, false).unwrap();

        let status = index_health(&data, &repos).unwrap();
        assert!(status.present && status.readable);
        assert!(status.stale.is_empty(), "freshly indexed repo is current");

        // A repo never indexed is stale.
        let ghost = vec![("ghost".to_string(), api)];
        let mixed = index_health(&data, &ghost).unwrap();
        assert_eq!(mixed.stale, vec!["ghost".to_string()]);
    }
}
