## 1. Store baseline (per-repo mtime)

- [x] 1.1 Add an `indexed_mtime` column (unix seconds, integer, nullable/default) to the `repos` table schema in `search/index.rs`; ensure `open_for_build` creates or additively migrates it on existing DBs.
- [x] 1.2 In `reconcile_repo`, write the repo's newest tracked-file mtime into `indexed_mtime` in the same transaction that writes `indexed_commit` (accept the baseline as a parameter so core stays the source of truth).
- [x] 1.3 Add a store read `indexed_mtimes() -> HashMap<String, Option<i64>>` (or fold into the existing `indexed_commits` read) for the gate to consume.
- [x] 1.4 Unit test: `reconcile_repo` persists a baseline; a fresh DB has `None` for unknown repos.

## 2. mtime staleness gate (core)

- [x] 2.1 Add a `tracked_mtimes(repo, repo_path) -> Result<Option<i64>, git2::Error>` helper in `chunk.rs` that walks git-tracked entries and returns the max `metadata().modified()` (unix secs) without reading/hashing contents; skip staged-deleted/unreadable entries.
- [x] 2.2 Unit test: a repo with a freshly-touched tracked file reports a newer mtime than one recorded earlier; untracked/ignored files do not affect the result.

## 3. refresh_stale entry point (core)

- [x] 3.1 Add `RefreshOutcome` (repos refreshed, files reprocessed, whether a full build occurred) and `refresh_stale(data_dir, repos, semantic, progress) -> Result<RefreshOutcome, RepographError>` in `search/mod.rs`: read baselines, compute the stale subset via `tracked_mtimes` (stale = newer than baseline OR no baseline OR no index), call `build_index` over only that subset, return the outcome.
- [x] 3.2 Ensure a missing index DB routes through a full build (all in-scope repos stale) rather than erroring.
- [x] 3.3 Unit/integration test (real `git2` + tempdir): edit an uncommitted tracked file → `refresh_stale` reindexes exactly that repo; second call with no change reindexes nothing and reads no contents.

## 4. Per-repo progress callback (core)

- [x] 4.1 Thread an optional progress hook (`&mut dyn FnMut(usize, usize, &str)` or a tiny `Progress` trait) through `build_index`'s repo loop; invoke it per repo with `(index, total, repo_name)`. No `indicatif` in core.
- [x] 4.2 Keep the existing `build_index` signature working for current callers (default no-op progress) or update all call sites in the same task.

## 5. `find` auto-refresh + `--no-refresh` (binary)

- [x] 5.1 Add `--no-refresh` to `commands/find.rs` `Args`.
- [x] 5.2 Acceptance test (`assert_cmd`, real repos in tempdir): edit an uncommitted file, run `find` without `--no-refresh`, assert the new content appears in results; assert `--no-refresh` does not pick it up.
- [x] 5.3 In `find::run`, unless `--no-refresh`, call `refresh_stale` over the same repo scope the query uses, before `search`; render refreshed-repos / already-fresh diagnostics to stderr.
- [x] 5.4 Missing index: default path builds then queries (exit 0); `--no-refresh` preserves the existing "no index exists" message + exit 3.
- [x] 5.5 Acceptance test: `find` before any `index` exits 0 and returns results; `find --no-refresh` before any index exits 3 with the guidance message.

## 6. `index` progress (binary)

- [x] 6.1 Pass a progress callback from `commands/index.rs` that updates the `indicatif` spinner message to `Indexing <name> (i/total)…`; keep it stderr-only and cleared before the summary.
- [x] 6.2 Manual check: a multi-repo `index` shows the repo name advancing; non-TTY still prints no spinner and pipes cleanly.

## 7. Docs & verification

- [x] 7.1 Update `README.md`: `find` auto-refresh behavior, `--no-refresh` flag, and the exit-code note (missing index no longer implies exit 3 by default).
- [x] 7.2 `cargo clippy -- -D warnings` clean; `cargo test` green (both crates).
- [x] 7.3 Verify JSON output of `find` is unchanged (envelope shape stable) with and without refresh.
- [x] 7.4 Run `openspec validate auto-index-refresh` and confirm `design.md` matches what was built (note any resolved deviations).
