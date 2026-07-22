## Why

repograph's search index is consumed mainly by AI agents that edit files and then search — often before committing. Today the index is only built when the user manually runs `repograph index`, so `find` returns stale results until someone remembers to reindex, and that command shows a single static spinner with no per-repo progress (a 30s+ run over a real registry is indistinguishable from a hang). Agents (and humans) expect `find` to reflect the current working tree, including uncommitted edits, without a manual reindex step.

## What Changes

- `repograph find` auto-refreshes the index before querying. For each in-scope repo it runs a cheap **mtime staleness check** — `stat` the git-tracked files (no reads) and compare the newest mtime against the mtime recorded at last index — and incrementally reindexes only repos whose working tree changed (or a missing index), then queries. This catches **uncommitted** working-tree edits, not just commits.
- Each repo's newest-tracked-file mtime at index time is recorded alongside the existing indexed-commit record, so the staleness gate has a baseline to compare against.
- A `--no-refresh` flag on `find` skips the auto-refresh (for scripts/agents wanting deterministic, index-only behavior).
- `repograph index` gains per-repo progress so a long run reports which repo it is on instead of a frozen spinner.
- Auto-refresh diagnostics (which repos were refreshed, or that the index was already fresh) go to stderr; the stdout data contract for `find` is unchanged.

## Capabilities

### New Capabilities
<!-- none -->

### Modified Capabilities
- `cross-repo-search`: `find` gains mtime-gated auto-refresh (catching uncommitted edits) and a `--no-refresh` opt-out; the index records a per-repo mtime baseline; `index` gains per-repo progress reporting. The "Find before any index exists" behavior changes — a missing index is auto-built rather than erroring with exit 3 (unless `--no-refresh` is set).

## Impact

- Code: `crates/repograph/src/commands/find.rs` (auto-refresh orchestration, `--no-refresh` flag), `crates/repograph/src/commands/index.rs` (per-repo progress), `crates/repograph-core/src/search/mod.rs` + `index.rs` (a refresh entry point that mtime-gates then calls `build_index`; store the per-repo newest-mtime baseline; possibly a `build_index` progress callback), `crates/repograph-core/src/search/chunk.rs` (expose tracked-file mtimes for the gate).
- Behavior: `repograph find` on a stale/missing index now performs I/O (incremental reindex) before returning; the common fresh-index case adds only a per-repo `stat` sweep of tracked files (tens of ms).
- Docs: README `find` section and exit-code notes (missing-index no longer implies exit 3 by default).
- No new dependencies. Semantic embeddings remain opt-in and unaffected.
