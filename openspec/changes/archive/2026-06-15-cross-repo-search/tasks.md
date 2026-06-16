## 1. Dependencies & scaffolding

- [x] 1.1 Add `rusqlite` (features = ["bundled"], FTS5 enabled) and `fastembed` (optional, behind `semantic` feature) to `repograph-core/Cargo.toml` (deny check deferred to 9.2)
- [x] 1.2 Create `crates/repograph-core/src/search/mod.rs` and register the module in `lib.rs`
- [x] 1.3 Data-dir layout: `INDEX_DB_NAME`/`MODEL_SUBDIR` constants + `index_db_path`/`model_cache_dir` in `search/mod.rs`; binary resolves the data dir (mirroring `--config-dir`) and passes a path to core

## 2. Acceptance tests first (outside-in TDD)

- [x] 2.1 Acceptance test (`tests/index.rs`): `repograph index` populates the index and exits 0; untracked files excluded
- [x] 2.2 Acceptance test (`tests/find.rs`): `repograph find "<exact symbol>"` returns the planted chunk via lexical match, exit 0
- [~] 2.3 Fuzzy/semantic ranking: covered at the core layer (`search::tests`); the binary-level semantic test is deferred with the `semantic` feature (needs model download/network). Lexical fuzzy matching is exercised via the smoke test.
- [x] 2.4 Acceptance test (`tests/find.rs`): `find --json` envelope (`schema_version`/`query`/`hits[]` with repo/path/line/score/snippet), parses, `--limit` bounds, empty-results exit 0
- [x] 2.5 Acceptance test: `find` before any index → exit 3 + `repograph index` guidance; corrupt index → exit 1; lexical-fallback notice path implemented (`degraded` → stderr)
- [x] 2.6 Acceptance test (`tests/index.rs`): incremental reindex reprocesses only changed files; deleted/replaced content purged; unchanged run reports up to date
- [x] 2.7 Acceptance test (`tests/doctor.rs`): index check reports ok / warn(missing) / warn(stale + repo name) without panicking
- [x] 2.8 Acceptance test (`tests/init_artifacts.rs`): artifact body contains `repograph find` cross-repo guidance

## 3. Chunking (`search/chunk.rs`)

- [x] 3.1 Enumerate git-tracked files for a repo via `git2`, skipping ignored/untracked and non-UTF-8/over-size files (named size cap)
- [x] 3.2 Split a file into bounded line-window chunks with overlap (named constants); attach `repo › relpath › lines` contextual prefix and start line
- [x] 3.3 Compute a per-file content hash used for incremental invalidation (git blob SHA, no extra dep)

## 4. Index engine (`search/index.rs`)

- [x] 4.1 Open/create `index.db`; schema: `files(repo,path,content_hash)`, `repos(repo,indexed_commit)`, `chunks`, FTS5 `chunks_fts`, `vectors(chunk_id,embedding,model)`, `meta(key,value)`
- [x] 4.2 Upsert path: insert chunks + FTS rows in one transaction per repo
- [x] 4.3 Incremental path: diff content hash, reprocess only changed files, purge deleted files, record per-repo indexed commit
- [x] 4.4 Schema-version guard (rebuild on mismatch) + model guard (clear vectors on model change)
- [x] 4.5 Lexical query (FTS5/BM25) returning ranked chunk ids, optional repo filter
- [x] 4.6 Vector query (brute-force cosine) + reciprocal-rank fusion of lexical + vector rankings

## 5. Embeddings (`search/embed.rs`)

- [x] 5.1 `fastembed` init behind `semantic` feature, model cached under data-dir subdir; init failure degrades to lexical with a notice (feature-on build unverified this session — needs network for ONNX + model)
- [x] 5.2 Embed chunk index-text to Float32 vectors; serialize/deserialize as BLOBs matching the `vectors` schema

## 6. Errors & core API

- [x] 6.1 `RepographError::IndexMissing` (→ exit 3) and `Index(String)` (→ exit 1) with `From<rusqlite::Error>`; exit-code tests added
- [x] 6.2 Public core API: `build_index`, `search`, `index_health`, `index_db_path`, `model_cache_dir`, `Hit`, `IndexStatus`, `IndexOutcome`, `SearchOutcome` re-exported from `lib.rs`

## 7. CLI commands (presentation only)

- [x] 7.1 `commands/index.rs`: clap `Args` (`--workspace`, `--semantic`), `run()` with `tracing` + stderr spinner/summary, exit 0; `--data-dir`/`REPOGRAPH_DATA_DIR` global resolved in `main.rs` and wired
- [x] 7.2 `commands/find.rs`: clap `Args` (`query`, `--workspace`, `--limit`, `--semantic`, `--json`), `OutputMode` once, TTY table vs JSON envelope via `output.rs::render_hits`, exit codes mapped; wired into `main.rs`

## 8. Doctor & agent artifact integration

- [x] 8.1 `Check::SearchIndex` + `DoctorReport::with_index_check` (ok / warn-missing / warn-stale-with-repo); binary computes `index_health` and folds it in; renderer label added
- [x] 8.2 Added `repograph find` cross-repo guidance (when-to-invoke bullet + commands rows + note) to `agent_artifact_body.md`

## 9. Docs & verification

- [x] 9.1 Documented `index`/`find` (flags, JSON shape, exit codes) + `--data-dir` in `README.md`; Commands, JSON-shapes, and exit-code sections updated
- [x] 9.2 `cargo clippy --workspace --all-targets -- -D warnings` clean (incl. `--features semantic`); `cargo test --workspace` green (~470 tests); `cargo deny check` ok; manual `find --json | jq` + exit-3 verified. (`cargo dist plan` not run — cargo-dist is CI-only and no release config changed.)
- [x] 9.3 `openspec validate cross-repo-search` passes
