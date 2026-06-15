//! The SQLite-backed search store.
//!
//! One central database (a `repo` column on every row) so a single query spans
//! all registered repos. FTS5 provides BM25 lexical search; a `vectors` table
//! holds Float32 embedding BLOBs for brute-force cosine. Lexical and vector
//! rankings are merged by reciprocal-rank fusion in [`fuse`]. Embeddings are
//! supplied through the [`Embedder`] trait so this module never depends on the
//! `fastembed` crate directly.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{Connection, OpenFlags, params, params_from_iter};

use crate::error::RepographError;
use crate::search::chunk::{Chunk, TrackedFile, chunk_file};

/// Bumped whenever the on-disk schema changes shape. A mismatch triggers a
/// clean rebuild rather than a fragile migration — the index is a derived
/// artifact, cheap to recreate.
pub const SCHEMA_VERSION: &str = "1";

/// Reciprocal-rank-fusion constant. 60 is the value from the original RRF paper
/// and the de-facto default; it damps the contribution of low-ranked hits.
const RRF_K: f64 = 60.0;

/// An embedding backend. Implemented by the (feature-gated) `embed` module; the
/// store takes it as a trait object so the always-on lexical path pulls in no
/// embedding dependency.
pub trait Embedder {
    /// Stable identifier of the model, stored alongside vectors so a model
    /// change invalidates the vector segment.
    fn model_id(&self) -> &str;

    /// Embed a batch of texts into vectors. Returns a human-readable message on
    /// failure; the caller degrades to lexical rather than aborting.
    ///
    /// # Errors
    ///
    /// Returns `Err(message)` when the backend cannot produce embeddings.
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, String>;
}

/// Per-repo outcome of [`Store::reconcile_repo`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RepoStats {
    /// Files (re)chunked because they were new or changed.
    pub files_indexed: usize,
    /// Files left untouched because their content hash matched.
    pub files_unchanged: usize,
    /// Files dropped from the index because they are no longer tracked.
    pub files_purged: usize,
}

/// A chunk row materialized for output.
#[derive(Debug, Clone)]
pub struct ChunkRow {
    pub repo: String,
    pub path: String,
    pub start_line: u32,
    pub content: String,
}

/// Handle to the search database.
pub struct Store {
    conn: Connection,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Store {
    /// Open the index for *building*, creating the file and schema if absent.
    /// A schema-version mismatch wipes and recreates all tables.
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::Index`] on any `SQLite` failure.
    pub fn open_for_build(db_path: &Path) -> Result<Self, RepographError> {
        if let Some(parent) = db_path.parent() {
            fs_err::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        let store = Self { conn };
        store.ensure_schema()?;
        Ok(store)
    }

    /// Open an *existing* index read-only. Both callers (`search`,
    /// `index_health`) only query, so a read-only handle avoids write-lock
    /// contention with a concurrent `repograph index` and works on read-only
    /// mounts. Returns [`RepographError::IndexMissing`] (exit 3) when the file
    /// does not exist, and [`RepographError::Index`] (exit 1) when it exists but
    /// cannot be opened or is the wrong schema.
    ///
    /// # Errors
    ///
    /// See above.
    pub fn open_existing(db_path: &Path) -> Result<Self, RepographError> {
        if !db_path.is_file() {
            return Err(RepographError::IndexMissing);
        }
        let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let store = Self { conn };
        let version: Option<String> = store.meta_get("schema_version")?;
        match version.as_deref() {
            Some(v) if v == SCHEMA_VERSION => Ok(store),
            Some(other) => Err(RepographError::Index(format!(
                "index schema version {other} is not readable by this build (expected {SCHEMA_VERSION}); run `repograph index` to rebuild"
            ))),
            None => Err(RepographError::Index(
                "index is missing its schema marker (corrupt); run `repograph index` to rebuild"
                    .to_string(),
            )),
        }
    }

    fn ensure_schema(&self) -> Result<(), RepographError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        )?;
        let version: Option<String> = self.meta_get("schema_version")?;
        if version.as_deref() == Some(SCHEMA_VERSION) {
            return Ok(());
        }
        if version.is_some() {
            self.drop_all()?;
        }
        self.create_all()?;
        self.meta_set("schema_version", SCHEMA_VERSION)?;
        Ok(())
    }

    fn drop_all(&self) -> Result<(), RepographError> {
        self.conn.execute_batch(
            "DROP TABLE IF EXISTS chunks_fts;
             DROP TABLE IF EXISTS vectors;
             DROP TABLE IF EXISTS chunks;
             DROP TABLE IF EXISTS files;
             DROP TABLE IF EXISTS repos;",
        )?;
        Ok(())
    }

    fn create_all(&self) -> Result<(), RepographError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS repos (
                 repo TEXT PRIMARY KEY,
                 indexed_commit TEXT
             );
             CREATE TABLE IF NOT EXISTS files (
                 repo TEXT NOT NULL,
                 path TEXT NOT NULL,
                 content_hash TEXT NOT NULL,
                 PRIMARY KEY (repo, path)
             );
             CREATE TABLE IF NOT EXISTS chunks (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 repo TEXT NOT NULL,
                 path TEXT NOT NULL,
                 start_line INTEGER NOT NULL,
                 end_line INTEGER NOT NULL,
                 content TEXT NOT NULL,
                 prefix TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_chunks_repo_path ON chunks(repo, path);
             CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(text, chunk_id UNINDEXED);
             CREATE TABLE IF NOT EXISTS vectors (
                 chunk_id INTEGER PRIMARY KEY,
                 embedding BLOB NOT NULL,
                 model TEXT NOT NULL
             );",
        )?;
        Ok(())
    }

    fn meta_get(&self, key: &str) -> Result<Option<String>, RepographError> {
        match self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| {
                r.get::<_, String>(0)
            }) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn meta_set(&self, key: &str, value: &str) -> Result<(), RepographError> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// If `model_id` differs from the model recorded in the index, drop every
    /// vector so the segment never mixes embedding spaces, then record the new
    /// model. Call once before reconciling with an embedder.
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::Index`] on `SQLite` failure.
    pub fn ensure_model(&self, model_id: &str) -> Result<(), RepographError> {
        let current: Option<String> = self.meta_get("model_id")?;
        if current.as_deref() != Some(model_id) {
            self.conn.execute("DELETE FROM vectors", [])?;
            self.meta_set("model_id", model_id)?;
        }
        Ok(())
    }

    /// Whether any embeddings are stored — drives whether semantic retrieval can
    /// run at query time.
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::Index`] on `SQLite` failure.
    pub fn has_vectors(&self) -> Result<bool, RepographError> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM vectors", [], |r| r.get(0))?;
        Ok(n > 0)
    }

    /// The per-repo indexed commit recorded at the last build.
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::Index`] on `SQLite` failure.
    pub fn indexed_commits(&self) -> Result<HashMap<String, Option<String>>, RepographError> {
        let mut stmt = self
            .conn
            .prepare("SELECT repo, indexed_commit FROM repos")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (repo, commit) = row?;
            out.insert(repo, commit);
        }
        Ok(out)
    }

    /// Reconcile one repo's tracked files against the index in a single
    /// transaction: re-chunk new/changed files, purge files no longer tracked,
    /// and record the indexed commit. When `embedder` is supplied, changed
    /// chunks are embedded and their vectors written; an embed failure for a
    /// file degrades that file to lexical-only (logged by the caller).
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::Index`] on `SQLite` failure.
    pub fn reconcile_repo(
        &mut self,
        repo: &str,
        files: &[TrackedFile],
        head_commit: Option<&str>,
        mut embedder: Option<&mut dyn Embedder>,
    ) -> Result<RepoStats, RepographError> {
        let mut stats = RepoStats::default();
        let existing = self.existing_hashes(repo)?;
        let embedding = embedder.is_some();
        // When embedding, a file whose content is unchanged but which carries no
        // stored vectors must still be reprocessed. Otherwise upgrading a lexical
        // index with `--semantic` (or switching models, which drops every vector
        // via `ensure_model`) would skip all unchanged files and leave the index
        // permanently half-embedded — the `--semantic` flag would silently no-op.
        let vectored: HashSet<String> = if embedding {
            self.paths_with_vectors(repo)?
        } else {
            HashSet::new()
        };
        let current: HashSet<&str> = files.iter().map(|f| f.path.as_str()).collect();

        let tx = self.conn.transaction()?;

        for path in existing.keys() {
            if !current.contains(path.as_str()) {
                delete_file_chunks(&tx, repo, path)?;
                tx.execute(
                    "DELETE FROM files WHERE repo = ?1 AND path = ?2",
                    params![repo, path],
                )?;
                stats.files_purged += 1;
            }
        }

        for f in files {
            let unchanged = existing.get(&f.path) == Some(&f.content_hash);
            let needs_vectors = embedding && !vectored.contains(&f.path);
            if unchanged && !needs_vectors {
                stats.files_unchanged += 1;
                continue;
            }
            delete_file_chunks(&tx, repo, &f.path)?;
            let chunks = chunk_file(repo, &f.path, &f.text);
            // Reborrow per iteration so the mutable borrow of `embedder` ends
            // each loop pass. `match` (not `.map()`) is required: the closure
            // form ties the reborrow to the whole fn and trips the borrow check.
            #[allow(clippy::option_if_let_else)]
            let emb: Option<&mut dyn Embedder> = match &mut embedder {
                Some(e) => Some(&mut **e),
                None => None,
            };
            let embeddings = embed_chunks(emb, &chunks);
            insert_chunks(&tx, repo, &chunks, embeddings.as_ref())?;
            tx.execute(
                "INSERT INTO files(repo, path, content_hash) VALUES(?1, ?2, ?3)
                 ON CONFLICT(repo, path) DO UPDATE SET content_hash = excluded.content_hash",
                params![repo, f.path, f.content_hash],
            )?;
            stats.files_indexed += 1;
        }

        tx.execute(
            "INSERT INTO repos(repo, indexed_commit) VALUES(?1, ?2)
             ON CONFLICT(repo) DO UPDATE SET indexed_commit = excluded.indexed_commit",
            params![repo, head_commit],
        )?;
        tx.commit()?;
        Ok(stats)
    }

    /// Repo-relative paths that currently have at least one stored embedding —
    /// used to detect files that are lexically indexed but not yet vectored.
    fn paths_with_vectors(&self, repo: &str) -> Result<HashSet<String>, RepographError> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT c.path FROM chunks c JOIN vectors v ON v.chunk_id = c.id
             WHERE c.repo = ?1",
        )?;
        let rows = stmt.query_map([repo], |r| r.get::<_, String>(0))?;
        let mut out = HashSet::new();
        for row in rows {
            out.insert(row?);
        }
        Ok(out)
    }

    fn existing_hashes(&self, repo: &str) -> Result<HashMap<String, String>, RepographError> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, content_hash FROM files WHERE repo = ?1")?;
        let rows = stmt.query_map([repo], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (path, hash) = row?;
            out.insert(path, hash);
        }
        Ok(out)
    }

    /// Lexical (BM25) candidate chunk ids, best-first. `repos` (when non-empty)
    /// restricts results to those repos. Returns an empty vec when the query
    /// yields no usable search tokens.
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::Index`] on `SQLite` failure.
    pub fn search_lexical(
        &self,
        query: &str,
        repos: &[String],
        pool: usize,
    ) -> Result<Vec<i64>, RepographError> {
        let Some(match_expr) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let pool_i = i64::try_from(pool).unwrap_or(i64::MAX);
        // FTS5's MATCH and bm25() must reference the virtual table by its real
        // name, not a join alias, so `chunks_fts` is spelled out here.
        let mut sql = String::from(
            "SELECT chunks.id FROM chunks_fts JOIN chunks ON chunks.id = chunks_fts.chunk_id
             WHERE chunks_fts MATCH ?1",
        );
        let mut binds: Vec<rusqlite::types::Value> = vec![match_expr.into()];
        if !repos.is_empty() {
            let placeholders = repo_placeholders(repos.len(), binds.len() + 1);
            sql.push_str(" AND chunks.repo IN (");
            sql.push_str(&placeholders);
            sql.push(')');
            for r in repos {
                binds.push(r.clone().into());
            }
        }
        sql.push_str(" ORDER BY bm25(chunks_fts) LIMIT ");
        sql.push_str(&pool_i.to_string());
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(binds), |r| r.get::<_, i64>(0))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row?);
        }
        Ok(ids)
    }

    /// Vector (cosine) candidate chunk ids, best-first, computed by brute force
    /// over the stored embeddings (optionally restricted to `repos`).
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::Index`] on `SQLite` failure.
    pub fn search_vectors(
        &self,
        query_embedding: &[f32],
        repos: &[String],
        pool: usize,
    ) -> Result<Vec<i64>, RepographError> {
        let mut sql = String::from(
            "SELECT v.chunk_id, v.embedding FROM vectors v JOIN chunks c ON c.id = v.chunk_id",
        );
        let mut binds: Vec<rusqlite::types::Value> = Vec::new();
        if !repos.is_empty() {
            let placeholders = repo_placeholders(repos.len(), 1);
            sql.push_str(" WHERE c.repo IN (");
            sql.push_str(&placeholders);
            sql.push(')');
            for r in repos {
                binds.push(r.clone().into());
            }
        }
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(binds), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?;
        let mut scored: Vec<(i64, f32)> = Vec::new();
        for row in rows {
            let (id, blob) = row?;
            let v = blob_to_vec(&blob);
            scored.push((id, cosine(query_embedding, &v)));
        }
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored.truncate(pool);
        Ok(scored.into_iter().map(|(id, _)| id).collect())
    }

    /// Fetch chunk rows for the given ids, keyed by id.
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::Index`] on `SQLite` failure.
    pub fn fetch_chunks(&self, ids: &[i64]) -> Result<HashMap<i64, ChunkRow>, RepographError> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders = repo_placeholders(ids.len(), 1);
        let sql = format!(
            "SELECT id, repo, path, start_line, content FROM chunks WHERE id IN ({placeholders})"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let binds: Vec<rusqlite::types::Value> = ids.iter().map(|i| (*i).into()).collect();
        let rows = stmt.query_map(params_from_iter(binds), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                ChunkRow {
                    repo: r.get::<_, String>(1)?,
                    path: r.get::<_, String>(2)?,
                    start_line: u32::try_from(r.get::<_, i64>(3)?).unwrap_or(u32::MAX),
                    content: r.get::<_, String>(4)?,
                },
            ))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (id, chunk) = row?;
            out.insert(id, chunk);
        }
        Ok(out)
    }
}

/// Merge ranked candidate lists by reciprocal-rank fusion, returning chunk ids
/// with their fused scores, best-first. An id appearing in multiple lists
/// accumulates contributions from each.
#[must_use]
pub fn fuse(lists: &[&[i64]]) -> Vec<(i64, f64)> {
    let mut scores: HashMap<i64, f64> = HashMap::new();
    for list in lists {
        for (rank, id) in list.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let contribution = 1.0 / (RRF_K + (rank as f64) + 1.0);
            *scores.entry(*id).or_insert(0.0) += contribution;
        }
    }
    let mut fused: Vec<(i64, f64)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    fused
}

fn embed_chunks(
    embedder: Option<&mut dyn Embedder>,
    chunks: &[Chunk],
) -> Option<(Vec<Vec<f32>>, String)> {
    let embedder = embedder?;
    if chunks.is_empty() {
        return None;
    }
    let texts: Vec<String> = chunks.iter().map(Chunk::index_text).collect();
    let model = embedder.model_id().to_string();
    match embedder.embed(&texts) {
        Ok(vectors) if vectors.len() == chunks.len() => Some((vectors, model)),
        Ok(_) => {
            tracing::warn!("embedder returned a vector count != chunk count; skipping vectors");
            None
        }
        Err(msg) => {
            tracing::warn!(error = %msg, "embedding failed; this file is lexical-only");
            None
        }
    }
}

fn delete_file_chunks(
    tx: &rusqlite::Transaction<'_>,
    repo: &str,
    path: &str,
) -> Result<(), RepographError> {
    tx.execute(
        "DELETE FROM chunks_fts WHERE chunk_id IN
             (SELECT id FROM chunks WHERE repo = ?1 AND path = ?2)",
        params![repo, path],
    )?;
    tx.execute(
        "DELETE FROM vectors WHERE chunk_id IN
             (SELECT id FROM chunks WHERE repo = ?1 AND path = ?2)",
        params![repo, path],
    )?;
    tx.execute(
        "DELETE FROM chunks WHERE repo = ?1 AND path = ?2",
        params![repo, path],
    )?;
    Ok(())
}

fn insert_chunks(
    tx: &rusqlite::Transaction<'_>,
    repo: &str,
    chunks: &[Chunk],
    embeddings: Option<&(Vec<Vec<f32>>, String)>,
) -> Result<(), RepographError> {
    for (i, chunk) in chunks.iter().enumerate() {
        tx.execute(
            "INSERT INTO chunks(repo, path, start_line, end_line, content, prefix)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                repo,
                chunk.path,
                chunk.start_line,
                chunk.end_line,
                chunk.content,
                chunk.prefix
            ],
        )?;
        let chunk_id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO chunks_fts(text, chunk_id) VALUES(?1, ?2)",
            params![chunk.index_text(), chunk_id],
        )?;
        if let Some((vectors, model)) = embeddings {
            if let Some(v) = vectors.get(i) {
                tx.execute(
                    "INSERT INTO vectors(chunk_id, embedding, model) VALUES(?1, ?2, ?3)",
                    params![chunk_id, vec_to_blob(v), model],
                )?;
            }
        }
    }
    Ok(())
}

/// Build an FTS5 MATCH expression from a free-form query: extract alphanumeric
/// tokens, lowercase, dedup, and OR them together (quoted, so FTS treats each as
/// a bare term). Returns `None` when the query has no usable tokens.
fn fts_query(query: &str) -> Option<String> {
    let mut seen = HashSet::new();
    let mut terms = Vec::new();
    for raw in query.split(|c: char| !c.is_alphanumeric()) {
        if raw.is_empty() {
            continue;
        }
        let lower = raw.to_lowercase();
        if seen.insert(lower.clone()) {
            terms.push(format!("\"{lower}\""));
        }
    }
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" OR "))
    }
}

/// `?(start), ?(start+1), …` for an `IN (...)` clause of `n` items.
fn repo_placeholders(n: usize, start: usize) -> String {
    (start..start + n)
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for x in v {
        bytes.extend_from_slice(&x.to_le_bytes());
    }
    bytes
}

fn blob_to_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::float_cmp,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::unnecessary_literal_bound
    )]
    use super::*;
    use crate::search::chunk::TrackedFile;
    use tempfile::TempDir;

    /// Deterministic in-memory embedder for exercising the vector path without
    /// the `semantic` feature / a real model download.
    struct StubEmbedder;
    impl Embedder for StubEmbedder {
        fn model_id(&self) -> &str {
            "stub-v1"
        }
        fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
            Ok(texts
                .iter()
                .map(|t| vec![(t.len() % 7) as f32 + 1.0, 1.0, 0.5])
                .collect())
        }
    }

    fn tf(path: &str, text: &str) -> TrackedFile {
        TrackedFile {
            path: path.to_string(),
            content_hash: format!("h:{}:{}", path, text.len()),
            text: text.to_string(),
        }
    }

    fn build_store() -> (TempDir, Store) {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("repograph").join("index.db");
        let store = Store::open_for_build(&db).unwrap();
        (tmp, store)
    }

    #[test]
    fn open_existing_missing_is_index_missing() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("nope.db");
        let err = Store::open_existing(&db).unwrap_err();
        assert!(matches!(err, RepographError::IndexMissing));
    }

    #[test]
    fn reconcile_then_lexical_finds_exact_token() {
        let (_tmp, mut store) = build_store();
        let files = vec![
            tf("auth.rs", "fn rotate_refresh_token() { /* logic */ }\n"),
            tf("util.rs", "fn unrelated_helper() {}\n"),
        ];
        let stats = store
            .reconcile_repo("api", &files, Some("deadbeef"), None)
            .unwrap();
        assert_eq!(stats.files_indexed, 2);
        let ids = store
            .search_lexical("rotate_refresh_token", &[], 10)
            .unwrap();
        assert!(!ids.is_empty());
        let rows = store.fetch_chunks(&ids).unwrap();
        let hit = rows.values().find(|r| r.path == "auth.rs");
        assert!(hit.is_some(), "exact-symbol query surfaces the right file");
    }

    #[test]
    fn incremental_skips_unchanged_and_reprocesses_changed() {
        let (_tmp, mut store) = build_store();
        let files = vec![
            tf("a.rs", "fn first() {}\n"),
            tf("b.rs", "fn second() {}\n"),
        ];
        store.reconcile_repo("r", &files, None, None).unwrap();

        // Second run: a.rs unchanged, b.rs changed.
        let files2 = vec![
            tf("a.rs", "fn first() {}\n"),
            tf("b.rs", "fn second_renamed() {}\n"),
        ];
        let stats = store.reconcile_repo("r", &files2, None, None).unwrap();
        assert_eq!(stats.files_unchanged, 1, "a.rs hash matched");
        assert_eq!(stats.files_indexed, 1, "b.rs re-chunked");

        // The old symbol is gone, the new one is present.
        assert!(
            !store
                .search_lexical("second_renamed", &[], 10)
                .unwrap()
                .is_empty(),
            "new content searchable"
        );
        let old = store.search_lexical("second", &[], 10).unwrap();
        // "second" still tokenizes from "second_renamed"? No — token is the whole word.
        let rows = store.fetch_chunks(&old).unwrap();
        assert!(
            !rows.values().any(|r| r.content.contains("fn second()")),
            "stale chunk purged"
        );
    }

    #[test]
    fn semantic_upgrade_embeds_previously_lexical_files() {
        let (_tmp, mut store) = build_store();
        let files = vec![tf("a.rs", "fn a() {}\n"), tf("b.rs", "fn b() {}\n")];

        // First pass is lexical-only: no embedder, no vectors written.
        store.reconcile_repo("r", &files, None, None).unwrap();
        assert!(
            !store.has_vectors().unwrap(),
            "lexical build wrote no vectors"
        );

        // Re-run with an embedder over the *same, unchanged* files. Without the
        // missing-vector check this would skip every file and write no vectors.
        let mut emb = StubEmbedder;
        store.ensure_model(emb.model_id()).unwrap();
        let stats = store
            .reconcile_repo("r", &files, None, Some(&mut emb))
            .unwrap();
        assert_eq!(
            stats.files_indexed, 2,
            "unchanged-but-unvectored files are reprocessed to embed them"
        );
        assert_eq!(stats.files_unchanged, 0);
        assert!(
            store.has_vectors().unwrap(),
            "vectors present after the semantic upgrade"
        );

        // A third pass (still embedding) now finds vectors for every file and
        // skips them — no needless re-embedding once the index is whole.
        let mut emb2 = StubEmbedder;
        let stats2 = store
            .reconcile_repo("r", &files, None, Some(&mut emb2))
            .unwrap();
        assert_eq!(
            stats2.files_unchanged, 2,
            "fully-vectored files are skipped"
        );
        assert_eq!(stats2.files_indexed, 0);
    }

    #[test]
    fn purges_deleted_files() {
        let (_tmp, mut store) = build_store();
        store
            .reconcile_repo("r", &[tf("gone.rs", "fn doomed() {}\n")], None, None)
            .unwrap();
        assert!(!store.search_lexical("doomed", &[], 10).unwrap().is_empty());
        // gone.rs no longer tracked.
        let stats = store.reconcile_repo("r", &[], None, None).unwrap();
        assert_eq!(stats.files_purged, 1);
        assert!(store.search_lexical("doomed", &[], 10).unwrap().is_empty());
    }

    #[test]
    fn repo_filter_scopes_results() {
        let (_tmp, mut store) = build_store();
        store
            .reconcile_repo("api", &[tf("a.rs", "fn shared_thing() {}\n")], None, None)
            .unwrap();
        store
            .reconcile_repo("ui", &[tf("b.rs", "fn shared_thing() {}\n")], None, None)
            .unwrap();
        let all = store.search_lexical("shared_thing", &[], 10).unwrap();
        assert_eq!(all.len(), 2);
        let scoped = store
            .search_lexical("shared_thing", &["api".to_string()], 10)
            .unwrap();
        let rows = store.fetch_chunks(&scoped).unwrap();
        assert!(rows.values().all(|r| r.repo == "api"));
    }

    #[test]
    fn indexed_commits_recorded() {
        let (_tmp, mut store) = build_store();
        store
            .reconcile_repo("r", &[tf("a.rs", "fn a() {}\n")], Some("c0ffee"), None)
            .unwrap();
        let commits = store.indexed_commits().unwrap();
        assert_eq!(commits.get("r"), Some(&Some("c0ffee".to_string())));
    }

    #[test]
    fn fuse_rewards_agreement() {
        // id 2 appears high in both lists; id 1 only in lexical, id 3 only in vector.
        let lexical = [1i64, 2, 4];
        let vector = [2i64, 3, 4];
        let fused = fuse(&[&lexical, &vector]);
        assert_eq!(fused[0].0, 2, "id present in both lists ranks first");
    }

    #[test]
    fn fts_query_extracts_tokens() {
        assert_eq!(fts_query("  !!  "), None);
        assert_eq!(
            fts_query("Rotate Refresh"),
            Some("\"rotate\" OR \"refresh\"".to_string())
        );
    }

    #[test]
    fn cosine_identical_is_one() {
        let v = [1.0f32, 2.0, 3.0];
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn blob_round_trips() {
        let v = vec![0.5f32, -1.0, 3.25];
        assert_eq!(blob_to_vec(&vec_to_blob(&v)), v);
    }

    #[test]
    fn schema_version_mismatch_triggers_rebuild() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("index.db");
        {
            let mut store = Store::open_for_build(&db).unwrap();
            store
                .reconcile_repo("r", &[tf("a.rs", "fn keep() {}\n")], None, None)
                .unwrap();
            store.meta_set("schema_version", "0").unwrap();
        }
        // Reopening for build sees the stale version and wipes.
        let store = Store::open_for_build(&db).unwrap();
        assert!(
            store.search_lexical("keep", &[], 10).unwrap().is_empty(),
            "stale-schema index was rebuilt empty"
        );
    }
}
