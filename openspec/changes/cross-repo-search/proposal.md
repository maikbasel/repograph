## Why

Developers working across many repos repeatedly hit a recall problem: "I solved something like this before — in another repo I may not even be able to name." Today nothing in the registry can answer it. `repograph context` inlines agent docs, but there is no way to retrieve *code that is semantically similar to what you are describing* across all registered repos. Per-repo tools (e.g. codegraph) only see one codebase at a time; the cross-repo layer is exactly repograph's niche.

## What Changes

- Add `repograph index [--workspace <name>] [--semantic]` — builds/refreshes a search index over the git-tracked files of registered repos. Git-aware incremental: only files whose `content_hash` changed since the last indexed commit are re-processed.
- Add `repograph find "<query>" [--workspace <name>] [--limit <n>] [--json]` — returns ranked code hits (`repo · path · line · score · snippet`) across all registered repos (or one workspace). TTY renders a table; non-TTY/`--json` emits a stable envelope.
- **Hybrid retrieval**: BM25 lexical (SQLite FTS5) ∪ semantic vector similarity (local `fastembed` embeddings), merged by reciprocal-rank fusion. Semantic is opt-in (`--semantic` / config); lexical works with zero model.
- **Central index store**: one SQLite database at `dirs::data_dir()/repograph/index.db` with a `repo` column so a single query spans every registered repo. Embedding model cached once under `dirs::data_dir()/repograph/models/`.
- Chunking is language-agnostic line-windows with a contextual prefix (`repo › relpath › lines`). Tree-sitter symbol-aware chunking is explicitly **out of scope** for this change.
- `repograph doctor` gains an index-health check: index present, and not stale relative to each repo's HEAD.
- The per-agent instruction artifact (`agent-skills`) gains a line teaching agents to call `repograph find` for "I solved this before / it's somewhere in another repo" queries.
- No MCP server. Delivery stays CLI + the existing skill/`AGENTS.md` artifact.

## Capabilities

### New Capabilities
- `cross-repo-search`: indexing and hybrid (lexical + semantic) retrieval of code across all registered repos, exposed via `repograph index` and `repograph find`, with a stable JSON output contract.

### Modified Capabilities
- `doctor-command`: adds a check that the search index exists and is not stale relative to registered repos' HEAD commits.
- `agent-skills`: the generated instruction artifact gains guidance teaching agents when to invoke `repograph find`.

## Impact

- **New code**: `crates/repograph-core/src/search/` (chunking, embedding, SQLite index engine — no clap, no stdout); `crates/repograph/src/commands/find.rs` and `index.rs` (presentation, `OutputMode`, exit codes).
- **Modified code**: `doctor.rs` (new check), `agent_artifact_body.md` (new instruction line), clap command tree in `main.rs`, `error.rs` (new `RepographError` variants).
- **New dependencies**: `rusqlite` (bundled SQLite, FTS5 feature), `fastembed` (local ONNX embeddings). Both keep repograph fully offline after a one-time model download on first semantic index.
- **New on-disk artifact**: `index.db` + cached model under `dirs::data_dir()/repograph/` (data dir, kept separate from the TOML config in config dir).
- **Exit codes** (per existing contract): empty results = `0` (not an error); corrupt/unreadable index = `1`; bad arguments = `2`.
