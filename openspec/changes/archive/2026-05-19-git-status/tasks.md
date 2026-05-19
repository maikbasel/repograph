## 1. Outside-in acceptance test scaffolding

- [x] 1.1 Add `crates/repograph/tests/status.rs` skeleton: shared `tempdir`-based helpers reused from existing test modules (init git repo with N commits on a branch, init bare repo, init unborn repo, build a detached HEAD, configure an upstream remote that mirrors a known state, run `repograph` with `REPOGRAPH_CONFIG_DIR` set to the temp dir, assert exit code + stderr regex + stdout JSON shape).
- [x] 1.2 Add a failing acceptance test for `status` happy path against a single clean repo on a tracked branch — asserts the JSON envelope contains exactly one entry with `state = "clean"`, `error = null`, and the documented field set; exit `0`.
- [x] 1.3 Add one failing acceptance test per coarse state — `dirty`, `detached`, `unborn`, `bare`, `missing` (path removed), `missing` (`.git` removed) — each constructed with real `git2` against a `tempdir`.
- [x] 1.4 Add a failing acceptance test for `status --workspace <name>` happy path: workspace with two live members and one dangling — JSON contains exactly the two live rows, dangling is silently skipped, exit `0`.
- [x] 1.5 Add a failing acceptance test for the names-XOR-workspace usage rule (`status foo --workspace acme` → exit `2`).
- [x] 1.6 Add a failing acceptance test for unknown positional name (`status ghost` against an empty registry → exit `3`).
- [x] 1.7 Add a failing acceptance test for the single-explicit-name vs batch exit-code split — broken repo in a batch exits `0`; the same broken repo addressed by single positional name exits `3`.
- [x] 1.8 Run `cargo test` and verify only the new tests fail (no compile errors anywhere else).

## 2. Domain layer — `RepoStatus` and `inspect()` in `repograph-core`

- [x] 2.1 Add a `RepoStatus` struct to `crates/repograph-core/src/git.rs` with fields `name: String`, `path: PathBuf`, `branch: Option<String>`, `upstream: Option<String>`, `ahead: u32`, `behind: u32`, `dirty: bool`, `staged: u32`, `unstaged: u32`, `untracked: u32`, `state: RepoState`, `error: Option<String>`. `#[derive(Debug, Clone, serde::Serialize)]`; field order is the documented JSON order.
- [x] 2.2 Add a `RepoState` enum with variants `Clean`, `Dirty`, `Detached`, `Unborn`, `Bare`, `Missing`. `#[derive(...)]` with `#[serde(rename_all = "lowercase")]`.
- [x] 2.3 Implement `inspect(name: &str, path: &Path, fetch: bool) -> RepoStatus` — opens the repo via `git2::Repository::open`, classifies state, walks `git2::Statuses` for the working-tree counters, computes ahead/behind via `git2::Repository::graph_ahead_behind` against the upstream from `branch_upstream_name`. On any `git2` or filesystem error attributable to the repo, returns a populated `RepoStatus` with `state = Missing` or `state = Bare` and `error = Some(msg)` — `inspect()` MUST NOT propagate errors out via `Result`; the failure surface is per-row, not per-call.
- [x] 2.4 Implement the optional fetch step inside `inspect()` when `fetch == true` and the repo is in `Clean`/`Dirty` state with a resolvable upstream: locate the remote via `branch.<name>.remote`, call `Remote::fetch` with default refspecs for that branch only. Fetch failures populate `error` and keep the rest of the row computed from pre-fetch state.
- [x] 2.5 Carve the staged-vs-unstaged-vs-untracked classification of `git2::Status` flags into a private helper (`classify(status: git2::Status) -> (bool /*staged*/, bool /*unstaged*/, bool /*untracked*/)`). Untracked counts use `StatusOptions::include_untracked(true)`; `.gitignored` is excluded.
- [x] 2.6 Add the optional `tracing::warn!` for detached HEADs from within `inspect()`'s caller (do not log from domain code per `.claude/rules/logging.md`); `inspect()` returns the short SHA on detached repos via a new `detached_sha: Option<String>` field on `RepoStatus` so the CLI layer can warn.
- [x] 2.7 Re-export the new public types from `crates/repograph-core/src/lib.rs` (`RepoStatus`, `RepoState`, `inspect`).
- [x] 2.8 Add unit tests in `crates/repograph-core/src/git.rs` covering clean, dirty, staged-only, unstaged-only, untracked-only, detached, unborn, bare, missing-path, missing-`.git`-dir, no-upstream, ahead-only, behind-only, ahead-and-behind. Each test builds the state with real `git2::Repository::init` and `Index`/`Tree` writes in a `tempdir`. No mocks.
- [x] 2.9 `cargo test -p repograph-core` green; `cargo clippy -p repograph-core -- -D warnings` clean.

## 3. CLI surface — `status` subcommand

- [x] 3.1 Create `crates/repograph/src/commands/status.rs` with `Args { names: Vec<String>, workspace: Option<String>, json: bool, fetch: bool }` (clap derive). Positional `names` is `#[arg(value_name = "NAME")]`; `--workspace`, `--json`, `--fetch` are long flags.
- [x] 3.2 Add `Status(commands::status::Args)` variant to the top-level `Command` enum in `crates/repograph/src/main.rs` and route to `commands::status::run`.
- [x] 3.3 In `run()`, validate the names-XOR-workspace rule early: when both `args.names` is non-empty and `args.workspace` is `Some`, return a `RepographError::UsageError` (exit `2`) before touching the config.
- [x] 3.4 Resolve scope: positional names → load config, look up each in `Config::repos()`, error `NotFound` (exit `3`) on the first miss, dedupe the resolved list; `--workspace` → `Config::resolve_workspace(name)?` and use the live members; neither → all `Config::repos()` alphabetical.
- [x] 3.5 Add `rayon` to `crates/repograph/Cargo.toml` if not already a transitive dependency; otherwise reuse the existing entry. Confirm with `cargo tree`.
- [x] 3.6 Implement parallel scan: `repos.par_iter().map(|repo| inspect(&repo.name, &repo.path, args.fetch)).collect::<Vec<_>>()`. After collection, sort by `name` for stable output regardless of completion order.
- [x] 3.7 Decide exit semantics: when `args.names.len() == 1` and that single repo's `RepoStatus.state == Missing`, return `RepographError::NotFound { kind: "repo", name }` after rendering nothing to stdout; otherwise the batch exits `0` even with populated `error` fields.
- [x] 3.8 Emit per-repo `warn!` lines for any `state == Missing` or any `error.is_some()` (whichever populated it — missing path, broken git dir, fetch failure). Use `tracing::warn!(repo = %name, err = %msg, "status: per-repo failure");`.
- [x] 3.9 Apply `#[tracing::instrument(skip(args), fields(names = ?args.names, workspace = args.workspace.as_deref().unwrap_or("<none>"), json = args.json, fetch = args.fetch))]` to `run()`. Entry `debug!`, success `info!(count = ...)`, error path `error!` per logging rules.
- [x] 3.10 Detached HEAD: when any returned `RepoStatus` carries a `detached_sha`, emit a `warn!` line per repo naming the short SHA.

## 4. Output rendering

- [x] 4.1 Extend `crates/repograph/src/output.rs` with a `StatusEnvelope { repos: Vec<RepoStatus> }` newtype derived `Serialize`, so the JSON path is a direct `serde_json::to_writer`.
- [x] 4.2 Implement `render_statuses(mode: OutputMode, statuses: &[RepoStatus]) -> Result<(), RepographError>`:
  - JSON mode: `serde_json::to_writer(stdout, &StatusEnvelope { repos: statuses.into() })` + trailing newline; field order is the order of definition on `RepoStatus`.
  - TTY mode: `comfy-table::Table::new()` with `UTF8_FULL`, columns `name`, `branch`, `upstream`, `ahead`, `behind`, `dirty`, `state`. `branch`/`upstream` render `null` as the literal `-` for readability; `dirty` renders as `yes`/`no`; `state` renders the lowercase enum name.
- [x] 4.3 Empty result handling: JSON mode writes exactly `{"repos":[]}` (one line, no extra whitespace beyond what `serde_json` emits); TTY mode renders a header-only table — mirrors `render_repos`'s empty-case decision.
- [x] 4.4 Implement a `with_progress<T, F>(items: &[T], label: impl Fn(&T) -> String, body: F) -> Vec<R>` helper that owns the `indicatif::MultiProgress` lifecycle: spawns one spinner per item on stderr, runs `body` in parallel via `rayon`, drops the `MultiProgress` (clearing spinners) before returning. Only active when stdout is a TTY; non-TTY runs `body` directly with no progress UI.
- [x] 4.5 Unit tests for the JSON envelope shape: empty (`{"repos":[]}`), populated with each `RepoState` variant, `error` field present-and-null on healthy rows, `error` field present-and-populated on `missing`/`bare`/fetch-failed rows. Assert byte-stable output for golden-comparison tests.

## 5. Wire-up, validation, error handling

- [x] 5.1 Verify the clap derive picks up `repograph status` with all four flags. Help text mentions: positional names, `--workspace`, `--json`, `--fetch`. Conflicting `--workspace` + positional names is documented.
- [x] 5.2 Confirm exit-code mapping end-to-end via the acceptance tests written in step 1: `0` success / batch with errors, `2` names+workspace conflict, `3` unknown name / unknown workspace / single-explicit-name missing repo, `1` malformed TOML on config load. No new exit codes.
- [x] 5.3 Confirm stdout/stderr discipline: every acceptance test redirects stdout to a buffer and asserts the bytes are either empty (error path before render) or parseable JSON (or a valid `comfy-table` rendering in TTY tests). Stderr is captured separately and asserted to carry the per-repo warning when one was expected.
- [x] 5.4 Run `rg "unwrap\(\)|expect\(|println!|eprintln!|todo!\(|unimplemented!\(" crates/repograph-core/src crates/repograph/src` to confirm production code has zero matches outside `#[cfg(test)]` blocks. Use `output.rs` for the only legitimate stdout/stderr writers.

## 6. Acceptance test fill-out (red → green)

- [x] 6.1 Flesh out every spec scenario from `specs/git-status/spec.md` as a discrete acceptance test in `crates/repograph/tests/status.rs`. Each scenario maps to one or more `#[test]` functions.
- [x] 6.2 Add a parallelism smoke test: register five repos with controlled `git2` state, run `status --json` twice in succession, assert the output JSON is byte-identical across runs (deterministic ordering after collection).
- [x] 6.3 Add a `--fetch` isolation test: two repos where one's upstream is a `file://` remote of another `tempdir` repo (real fetch works) and the other's upstream is a non-existent `file://` path (fetch fails); assert the failing repo's `error` field is populated and the working row reflects the post-fetch state.
- [x] 6.4 Add a zero-network test: assert `status --json` (no `--fetch`) never invokes a network operation — verify by setting the upstream to a path that would fail if opened (e.g. permission-denied) and confirming the command still succeeds without touching it.
- [x] 6.5 Add a tombstone-interaction test: register a repo, add it to a workspace, remove the repo via `repograph remove`, then run `status --workspace <name>` — assert the workspace's now-dangling member is silently skipped and the exit code is `0`. Confirms `registry-core`/`workspace-support` invariants are preserved.
- [x] 6.6 `cargo test --workspace` fully green; `cargo clippy --workspace --all-targets -- -D warnings` clean.

## 7. Documentation & manual validation

- [x] 7.1 `README.md` command surface table extended with the `status` subcommand and its four flags. Exit-code table unchanged.
- [x] 7.2 `README.md` example output blocks added: status TTY table with two clean and one dirty row, status JSON envelope, status with a `state: "missing"` row, `status --workspace acme --json` filtered output, `status --fetch` opt-in network call noted.
- [x] 7.3 Manual smoke test with `REPOGRAPH_CONFIG_DIR=$(mktemp -d)`: register two real git repos, run `status`, edit a file in one to dirty it, re-run, detach the HEAD of the other, re-run, delete one's directory, re-run, confirm the four observed states match the spec.
- [x] 7.4 `openspec validate git-status` reports "valid".

## 8. Archive readiness check (pre-archive only — do NOT archive in this change)

- [x] 8.1 Every checkbox in this `tasks.md` is ticked.
- [x] 8.2 `cargo check --workspace` warning-free, `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --workspace` green.
- [x] 8.3 `design.md` reflects what was actually built. Any deviations are recorded as "Resolved deviation" notes.
- [x] 8.4 `README.md` command surface + exit-code table match the implementation.
- [x] 8.5 `registry-core` and `workspace-support` archived tests still green; no edits to either archived spec.
- [x] 8.6 No `unwrap()` / `expect()` outside test code; no `println!` / `eprintln!` outside `output.rs`; no shell-out to `git`.
