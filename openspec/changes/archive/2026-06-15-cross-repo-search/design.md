## Context

repograph is a registry of local git repos that exposes paths, git state, and inlined agent docs as structured JSON. It owns the *cross-repo* layer; per-repo semantic tools (codegraph) own the *intra-repo* layer. There is currently no way to answer "I solved something like this before, somewhere across my repos." This change adds cross-repo precedent search.

The workspace splits on the presentation/logic boundary: `repograph-core` owns domain logic (no clap, no stdout), the `repograph` binary owns presentation. The output contract is non-negotiable — pure data on stdout, diagnostics on stderr — and every failure maps to a documented exit code. Config persists as TOML under `dirs::config_dir()`. This change introduces the first non-config persisted artifact: a search index.

A survey of codegraph's storage confirmed the target pattern: a single SQLite file per repo holding `nodes`/`edges`/`files` (with per-file `content_hash` for incremental reindex), an FTS5 virtual table for BM25, and a `vectors` table of Float32 BLOB embeddings (brute-force cosine in app code), with the ONNX model cached once in a shared global dir. We adopt this pattern, with one change: a single central DB spanning all repos rather than one DB per repo.

## Goals / Non-Goals

**Goals:**
- Retrieve code semantically similar to a natural-language or symbol query across all registered repos in one call.
- Hybrid retrieval: BM25 (FTS5) fused with semantic vector similarity via reciprocal-rank fusion.
- Fully offline after a one-time model download; lexical retrieval works with no model at all.
- Git-aware incremental indexing keyed on per-file content hash.
- Stable JSON output contract for downstream agents; correct exit codes.
- Reuse the existing core/binary split, `OutputMode`, `RepographError`, `tracing`, and skill-artifact delivery.

**Non-Goals:**
- Tree-sitter / symbol-aware chunking (line-window chunking only in v1; symbol chunking is a later change).
- Typed inter-repo edges (dependency/API-consumer graph).
- MCP server (delivery is CLI + the existing skill artifact).
- Remote/cloud embeddings, watch-mode auto-reindex, an ANN index.

## Decisions

**SQLite (`rusqlite`, bundled, FTS5) as the single store.** One embedded file, transactional, no server, FTS5 gives BM25 for free. Bundled feature compiles SQLite in — no system dependency. *Alternative: tantivy + a separate vector segment — rejected: two stores to keep consistent, an extra dependency, and SQLite already covers both lexical and vector needs at our scale.*

**Central DB with a `repo` column, not per-repo files.** A single `index.db` at `dirs::data_dir()/repograph/index.db` lets one query span all repos — essential for "I don't know which repo." Reindexing one repo is `DELETE WHERE repo = ?` + reinsert. It also avoids littering `.codegraph`-style files into the user's repos. *Alternative: per-repo DBs like codegraph — rejected: forces fan-out + merge across N files on every query and pollutes repos.*

**Vectors as Float32 BLOBs, brute-force cosine.** Matches codegraph; at tens of repos × thousands of chunks a linear scan over candidate vectors is sub-second. Restrict the cosine pass to the lexical-candidate set ∪ a vector top-k to bound work. *Alternative: an ANN index (hnsw) — deferred as premature optimization; revisit if scale demands.*

**`fastembed` for local embeddings, semantic opt-in.** Bundled ONNX small model, cached once under `dirs::data_dir()/repograph/models/`. Semantic is opt-in (`--semantic` / config); `find` falls back to lexical-only when no vectors exist and says so on stderr. *Alternative: API embeddings — rejected: breaks offline use and the CLI's no-network ethos.*

**Reciprocal-rank fusion to merge result sets.** Cheap, model-free, robust to incommensurable BM25 vs cosine scores: `score = Σ 1/(k + rank_i)`. *Alternative: a learned/weighted blend — rejected: needs tuning data we don't have.*

**Language-agnostic line-window chunks with a contextual prefix.** Each chunk is a bounded line window (with small overlap) prefixed by `repo › relpath › lines` before embedding/indexing, improving recall for fuzzy queries without per-language grammars. *Alternative: tree-sitter symbol chunks — deferred to a later change to keep v1 shippable across all languages.*

**Index lives in data dir, manifest tracks invalidation.** A small metadata table records schema version, embedding model id, and per-repo indexed commit. A model/schema change forces a clean rebuild; the per-repo indexed commit drives the `doctor` staleness check.

**`fastembed` is an optional cargo dependency behind a default-off `semantic` feature.** `fastembed` pulls the ONNX runtime (`ort`), which downloads native binaries at build time and a model at runtime — that would break offline/CI builds and bloat the default `cargo install`. Gating it keeps the always-on lexical core (SQLite FTS5) light and fully offline; optional deps are not compiled unless the feature is enabled. `--semantic` becomes both a build-time opt-in (the `semantic` feature) and a runtime opt-in (the flag): built without the feature, `--semantic` emits a stderr notice and degrades to lexical; built with it, it embeds. Release/dist builds enable the feature so packaged binaries ship semantic. The binary resolves the data dir (mirroring `--config-dir` via a `--data-dir` / `REPOGRAPH_DATA_DIR` override) and passes an explicit path to core — core performs no `dirs` lookups, consistent with how `config.rs` takes a `dir: &Path`. *Alternative: fastembed always-on — rejected: breaks offline builds and the no-network ethos, and forces every install to carry ONNX.* Content hashing for incremental reindex reuses git's blob SHA via `git2` (`Oid::hash_object`), avoiding a new hashing dependency.

## Risks / Trade-offs

- **First-run model download (~100MB+)** → lexical works immediately with no model; semantic is opt-in and the download happens once, cached globally; failure to fetch degrades to lexical with a stderr notice, never a hard error.
- **Brute-force cosine won't scale to huge corpora** → bound the cosine pass to candidates; manifest makes adding an ANN index later a non-breaking internal change.
- **Index drift vs working tree** → index only git-tracked content at HEAD; `doctor` reports staleness vs HEAD so drift is visible, not silent.
- **SQLite write contention if indexing runs concurrently** → indexing is a single-process serial operation; wrap a repo's reindex in one transaction; no concurrent-writer guarantee is offered.
- **Large/binary tracked files** → cap per-file size and skip files that aren't valid UTF-8 text, mirroring codegraph's `maxFileSize` guard.
- **New persisted artifact under data dir** → tests must target a `tempdir` data dir, never the real one (consistent with the existing config-in-tempdir rule).

## Migration Plan

Additive only — no existing behavior changes. New commands (`index`, `find`), new error variants, one new doctor check, one new line in the agent artifact body. On a schema/model-version mismatch the index self-rebuilds on next `repograph index`; there is no user-data migration. Rollback is removing the commands; `index.db` is disposable and can be deleted at any time.

## Resolved Deviations

- **`fastembed` gated behind an optional `semantic` cargo feature (default off).** Not in the original plan, which implied fastembed was always-on. Rationale: the ONNX runtime downloads native binaries at build time and a model at runtime, which breaks offline/CI builds and bloats the default `cargo install`. The always-on lexical core (SQLite FTS5) stays light and offline; `--semantic` is both a build-time and runtime opt-in. The default build is fully implemented and tested; the `semantic` feature compiles and is clippy-clean, but its runtime behavior (model download + embedding) is not exercised by the test suite (needs network).
- **Released binaries ship lexical-only.** Wiring the `semantic` feature into the cargo-dist release matrix (cross-compiling ONNX for `aarch64-linux`, `windows-msvc`, etc.) is a separate, higher-risk effort and was deliberately not done here. Semantic search is available via `cargo install repograph --features semantic` or a source build. Enabling it in the dist matrix is tracked as follow-up, not part of this change.
- **Content hashing uses git's blob SHA (`git2::Oid::hash_object`)** rather than a new hashing crate — same identity git uses, zero added dependencies.

## Open Questions

None blocking. Default chunk window size/overlap and the FTS5-vs-vector candidate counts for fusion will be tuned during implementation against the acceptance tests; defaults will be named constants, not magic numbers.
