## Why

Phases 1 and 2 (`registry-core`, `workspace-support`) made the registry navigable, but it still only describes repos at rest — *where* they are, not *what state they're in*. An agent that asks "which of the four repos in workspace `acme` have uncommitted work?" can't answer that from `repograph list`; a developer juggling thirty repos has the same blind spot at the start of every day. Phase 4 (`context`) layers on top of this — an agent context block that doesn't include branch, dirty/clean, and ahead/behind is missing the most actionable signals about a repo. Phase 3 makes the registry observable so Phase 4 has something real to inline.

## What Changes

- **NEW**: `repograph status [<name>...] [--workspace <name>] [--json] [--fetch]` — show working-tree and branch state for one repo, many named repos, all registered repos, or a `--workspace`-filtered subset. Default scope when no positional args and no flag is "all registered repos".
- **NEW**: Per-repo status fields (via `git2`): current branch (or detached HEAD with short SHA), upstream tracking (`origin/<branch>` or `None`), ahead/behind counts, dirty flag (any non-clean entry in `git2::Statuses`), staged/unstaged/untracked counts, and a coarse `state` enum (`clean` / `dirty` / `detached` / `unborn` / `bare` / `missing`).
- **NEW**: `--fetch` flag — opt-in `git2::Remote::fetch` against the upstream remote before computing ahead/behind. Off by default (zero-network guarantee preserved). Surfaces fetch failures per repo without aborting the batch.
- **NEW**: Parallel scan — repos are introspected concurrently via `rayon`'s default global pool. A per-repo `indicatif` spinner stack on stderr (TTY only) shows progress; spinners clear before stdout writes.
- **NEW**: Tombstone-aware behavior — a registered repo whose path no longer resolves to a git repo (filesystem deleted, no longer a git dir, permission denied) does not abort the batch. It surfaces as `state: "missing"` with a structured `error` string in JSON and a stderr `warn!` line. Exit code is `0` unless the user explicitly requested a single repo by name (then exit `3`).
- **NEW**: TTY rendering — `comfy-table` with columns `name`, `branch`, `upstream`, `ahead`, `behind`, `dirty`, `state`. Empty result → header-only table (mirrors `registry-core` `list`).
- **NEW**: JSON envelope — `{ "repos": [ { "name": ..., "path": ..., "branch": ..., "upstream": ..., "ahead": <u32>, "behind": <u32>, "dirty": <bool>, "staged": <u32>, "unstaged": <u32>, "untracked": <u32>, "state": "<enum>", "error": <string?> }, ... ] }`. Field order stable; `error` is `null` (not omitted) on healthy repos so agent consumers can branch on `repo.error != null` without a key-existence check.
- **NEW**: `--workspace <name>` filter composes with `workspace-support` — same resolution path as `list --workspace`; dangling members are silently skipped (parity with `list`, not `workspace show`); unknown workspace → exit `3`.
- **NEW**: Positional name selection — `repograph status foo bar baz` runs status for exactly those three. Unknown name → exit `3`. Names + `--workspace` is a usage error (exit `2`).
- **UNCHANGED**: `registry-core` and `workspace-support` behaviors. `status` is read-only on the config; no `[repo.*]` or `[workspace.*]` mutation. No new exit codes. The output contract (stdout = data, stderr = diagnostics) is preserved.

## Capabilities

### New Capabilities

- `git-status`: Read-only `git2` introspection across one or many registered repositories — branch, upstream, ahead/behind, dirty state, untracked/staged/unstaged counts, coarse repo-state enum, optional pre-status `fetch`, parallel scanning with progress, tombstone-aware error surfaces. Owns the `repograph status` command, the `repos[].state` enum's JSON contract, and the per-repo `error` field shape.

### Modified Capabilities

<!-- None. The registry-core spec (archived) and the workspace-support spec (archived) stay invariant. Status composes against both: it resolves repos by registry name and accepts the workspace-support filter without redefining either contract. -->

## Impact

- **Code**:
  - `crates/repograph-core/src/git.rs` — extends the existing `git2` adapter. Adds a `RepoStatus` struct, a `repo_state()` helper, an `inspect(path: &Path, fetch: bool) -> Result<RepoStatus, RepographError>` entry point, and unit tests against real `git2::Repository::init` temp dirs (clean, dirty, staged, untracked, detached, unborn, ahead/behind, missing path).
  - `crates/repograph-core/src/error.rs` — at most one new variant (`GitOperation { path, op, source }` for fetch/branch/status failures attributable to a specific repo without conflating them with `GitOpen`). Existing `NotFound`/`UsageError` cover the rest. No new exit codes.
  - `crates/repograph-core/src/lib.rs` — re-exports the new public types.
  - `crates/repograph/src/commands/status.rs` — new file. Hosts `Args` (clap derive: optional positional `names: Vec<String>`, `--workspace`, `--json`, `--fetch`), validates the names-vs-workspace mutual exclusion at handler entry, fans repos out across `rayon`, collects results, renders.
  - `crates/repograph/src/main.rs` — adds the `Status` variant to the top-level `Command` enum.
  - `crates/repograph/src/output.rs` — adds `StatusEntry` / `StatusEnvelope` view types, a `render_statuses(mode, &[StatusEntry])` renderer for both TTY and JSON, and a `with_progress(repos, |name| ...)` helper that wires `indicatif::MultiProgress` to stderr in TTY mode.
  - `crates/repograph/Cargo.toml` — adds `rayon` to dependencies if not already present. `indicatif` is already in-tree.
  - `crates/repograph/tests/status.rs` — new acceptance tests covering each spec scenario, using `tempdir` + real `git2::Repository::init` to construct clean, dirty, staged, untracked, detached, unborn, ahead, behind, and missing-path cases.
- **TOML schema**: no changes. `status` is read-only.
- **Exit codes**: no new codes. Reuses 0 / 2 / 3 / 1 per the existing contract.
- **README.md**: command surface table extended with `status`; example output blocks for the TTY table and JSON envelope; the `--fetch` opt-in network call is documented as the only command in the suite that touches the network.
- **Dependencies**: `rayon` added if absent; no other new crates. `git2`, `indicatif`, `comfy-table`, `serde`, `serde_json`, `tracing`, `thiserror`, `is-terminal`, `fs-err` all already in-tree.
- **Out of scope**: a `--porcelain` / `--short` output flag; `--ignored` toggling; submodule recursion; per-file diff output; `git fetch --all` across remotes (only the upstream of the current branch is fetched); concurrency tuning flags (`--jobs <N>`); writing status to a cache; an MCP tool wrapper around `status` (the future MCP crate consumes the core API, but the binding lives in that crate); effects on `context` (Phase 4 will inline status output, not the other way around); `doctor` (Phase 5).
