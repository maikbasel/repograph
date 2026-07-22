## Context

The cross-repo search index (`repograph index` → SQLite at `data_dir/index.db`, queried by `repograph find`) is built only on explicit user command. repograph is consumed mainly by AI agents in an edit-then-search loop, frequently against **uncommitted** working-tree changes, so a manually-refreshed index is stale exactly when it matters.

Existing machinery to build on:
- `build_index(data_dir, repos, semantic)` — already **incremental and git-aware**: per file it stores a git-blob content hash and re-chunks only changed files, purges deleted ones (`search/index.rs::reconcile_repo`). Re-running it over a scope where nothing changed is correct but not free: it re-reads and re-hashes every tracked file.
- `index_health(data_dir, repos)` — cheap staleness check comparing each repo's git **HEAD** against the stored `indexed_commit`.
- `chunk::tracked_files(repo, path)` — enumerates git-tracked files (already `stat`s each via `std::fs::metadata` for the size guard).
- The `repos` table stores `(repo, indexed_commit)`.

Constraint: HEAD-based staleness (what `index_health` gives) only flips after a commit, so it cannot detect uncommitted edits — the primary case. We need a working-tree-sensitive gate that stays cheap.

## Goals / Non-Goals

**Goals:**
- `repograph find` returns results reflecting the current working tree, including uncommitted edits, without a manual `index` step.
- The freshness gate is cheap in the common case (nothing changed): a `stat`-only sweep, no file reads, no chunking, no embedding.
- Auto-refresh is transparent: diagnostics on stderr, stdout data contract unchanged.
- A `--no-refresh` opt-out gives deterministic, index-only behavior for scripts.
- `repograph index` reports per-repo progress instead of a single static spinner.

**Non-Goals:**
- No file-watcher daemon and no git hooks (codegraph removed both).
- No Claude-Code-hook / dirty-flag mechanism in this change (documented as a possible future layer; not required since the mtime gate already catches uncommitted edits for every caller).
- No change to the retrieval algorithm, ranking, or the `find` JSON envelope shape.
- No change to semantic embeddings (still opt-in, still `--semantic`).

## Decisions

### Decision 1: mtime-gated auto-refresh inside `find` (not HEAD, not hooks)

At `find`, for each in-scope repo, compute the **newest mtime across its git-tracked files** and compare to a per-repo baseline stored at last index. If newer (or no baseline / no index), that repo is stale and gets an incremental reindex before the query runs.

**Why mtime over HEAD:** HEAD only moves on commit; agents search before committing. mtime reflects the working tree.

**Why mtime over content-hashing at gate time:** hashing every tracked file every `find` is the expensive part of a reindex — it defeats the point of a gate. `stat` is ~tens of ms for thousands of files. mtime decides *whether* to reindex; the reindex itself still content-hashes, so a false-positive mtime bump just triggers a cheap no-op reconcile (correct, slightly slower), never a wrong result.

**Why not the codegraph dirty-flag/hook:** it only fires inside Claude-Code sessions with the hook installed; a plain `repograph find` (human, CI, other agent runners) wouldn't refresh. The mtime gate is universal and needs no install step. (The hook remains a viable *additive* future optimization to skip the stat sweep in-session.)

**Known ceiling** (`ponytail:` in code): mtime can miss an edit that rewrites a file without advancing mtime (rare; some `git checkout`/restore operations reset mtimes backward, and coarse-mtime filesystems). Mitigation: `repograph index` remains a full, hash-authoritative refresh; document that `find --no-refresh` + explicit `index` is the deterministic path. This matches how every mtime-based build tool (make, etc.) behaves.

### Decision 2: store a per-repo mtime baseline in the `repos` table

Add an `indexed_mtime` column (unix seconds, integer) to the `repos` table, written in the same `reconcile_repo` transaction that writes `indexed_commit`. The baseline is the max tracked-file mtime observed during that index. Reuses the existing per-repo record; no new table.

Migration: additive column with a default; a pre-existing index without the column reads as "no baseline" → treated as stale on first `find` (one refresh), then populated. The index is derived and disposable, so a schema bump that forces one reindex is acceptable.

**Resolved deviation (implementation):** rather than an additive `ALTER TABLE` migration, the implementation bumps `SCHEMA_VERSION` (`"1"` → `"2"`) and adds `indexed_mtime` to `create_all`. The store already treats a version mismatch as drop-and-rebuild (its documented philosophy: "a mismatch triggers a clean rebuild rather than a fragile migration"), so reusing that path is less code than a bespoke `ALTER`. The one-time cost is a full rebuild of a pre-existing index on the first `index`/auto-refresh after upgrade (vs. the "one incremental refresh" the additive approach implied) — equivalent in user impact, since the index is derived and disposable. Note: under `--no-refresh`, an old-schema index surfaces as an `Index` error (exit 1) directing the user to rebuild, since auto-refresh (which would rebuild it) is suppressed.

### Decision 3: a core `refresh_stale` entry point; the binary orchestrates

Core gains `refresh_stale(data_dir, repos, semantic) -> RefreshOutcome` that: opens the store, reads baselines, computes the stale subset via the mtime sweep, calls `build_index` on just that subset, and returns which repos were refreshed. `find` calls it (unless `--no-refresh`) before `search`. Keeps all git2/SQLite in core per the architecture boundary; the binary only decides opt-out and renders stderr diagnostics.

`build_index` is reused as-is for the actual work (it already handles the incremental reconcile); `refresh_stale` is the scoping + gating wrapper. The mtime sweep reuses `tracked_files` (or a lighter `tracked_mtimes` helper that stops at `metadata` and skips the read/hash).

### Decision 4: per-repo progress for `index` (and refresh)

Thread an optional progress callback (`FnMut(&str)` with the repo name, or a small `Progress` trait) through `build_index`'s repo loop so the command layer can update the spinner message per repo (`Indexing taverne (3/9)…`). Core stays presentation-free — it invokes the callback; the binary owns the `indicatif` spinner. Fixes the "looks hung" report for both explicit `index` and auto-refresh.

### Decision 5: missing index auto-builds instead of exit 3

With auto-refresh on, `find` against a never-built index builds it, then queries, exiting 0 — the old "no index exists, run `repograph index`" (exit 3) only applies under `--no-refresh`. This is a behavior change to the `cross-repo-search` "Corrupt or unreadable index is surfaced" requirement's missing-index scenario; the corrupt-index path (exit 1) is unchanged.

## Risks / Trade-offs

- **mtime false-negative (missed edit)** → Mitigation: explicit `repograph index` is hash-authoritative; documented. Low frequency, matches make-style tooling.
- **mtime false-positive (needless reindex)** → Only cost is a cheap incremental reconcile that finds nothing to do; no correctness impact.
- **Latency added to `find`** → Common case is a stat-sweep (tens of ms). Worst case (large uncommitted change) is an incremental reindex of the affected repo only. `--no-refresh` escapes entirely for latency-sensitive callers.
- **Schema migration forces one reindex** on existing indexes → Acceptable: index is derived/disposable; one-time.
- **stat sweep on a huge repo** → bounded by tracked-file count (same walk `tracked_files` already does); no reads, so far cheaper than the reindex it gates.

## Migration Plan

1. Additive `indexed_mtime` column with a safe default (e.g. `0`/NULL) in the store's schema/`open_for_build` path; existing rows read as "no baseline" → stale-on-first-find.
2. No user action required; first `find` (or `index`) after upgrade repopulates baselines.
3. Rollback: reverting the binary leaves the extra column unused and harmless; `index`/`find` on the old binary ignore it.

## Open Questions

- Progress plumbing shape: a `FnMut(&str)` callback vs. a tiny `Progress` trait object — pick the lighter one that keeps core free of `indicatif`. (Resolve in implementation; not spec-visible.)
- Whether `refresh_stale` should honor `--workspace` scope identically to `find`'s existing repo filter — yes; it refreshes exactly the repos `find` will query. (Confirmed; noted here for the tasks phase.)
