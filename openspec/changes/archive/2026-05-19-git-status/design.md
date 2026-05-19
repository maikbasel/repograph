## Context

Phase 1 (`registry-core`, archived) gave us a flat, persistent registry of git repos. Phase 2 (`workspace-support`, archived) layered named groupings on top, plus a `--workspace` filter on `list`. Both are read-mostly catalogs: they answer *where* and *which*, not *what state*.

Phase 3 is the first capability that actually opens the repos it tracks. Status answers four questions a developer or agent will routinely ask:

- "Which of these have uncommitted work?"
- "Which branch am I on across the workspace?"
- "Have I forgotten to push something?"
- "Are any of these registered paths broken (deleted, no longer a git repo, permission-denied)?"

Constraints shaping this design:

- **`registry-core` and `workspace-support` specs are archived and stay invariant.** `status` is a *read-only* consumer of both. No `[repo.*]` writes, no `[workspace.*]` writes, no behavior changes to `add` / `list` / `remove` / `workspace`. The atomic-multi-repo and tombstone semantics from Phase 2 are reused, not re-litigated.
- **The output contract is a hard guarantee.** stdout is data (JSON envelope on `--json` or non-TTY, `comfy-table` on TTY). stderr is everything else (spinners, warnings, per-repo fetch errors, log lines). `tracing` already writes to stderr; `indicatif::MultiProgress` is wired through the same stderr channel and *must* be cleared before any stdout write.
- **Zero-network by default.** `repograph` is a developer/agent tool, not a CI probe. Fetching is opt-in (`--fetch`), bounded to the upstream of the current branch, and per-repo failures don't poison the batch.
- **`git2` exclusively.** No shelling out to `git`. Every operation that can fail is wrapped in `RepographError` with the right exit code.
- **Acceptance tests run against real `git2`.** No mocks. Each spec scenario is reproducible via `tempdir` + `git2::Repository::init` with deliberate state (commits, branches, working tree edits, detached HEADs, deleted paths).

Industry prior art was surveyed (`gita`, `mu-repo`, `vcsh`, `gitsum`, `lazygit`'s multi-repo mode, JetBrains' multi-VCS view, `git status --porcelain=v2`) and informed the column layout, the parallelism choice, and the failure semantics. The decisions below are where multiple reasonable approaches existed and we needed to pin one.

## Goals / Non-Goals

**Goals:**

- Ship `repograph status` end-to-end: positional name selection, `--workspace` filter, all-repos default, `--json` flag, TTY table + JSON envelope, parallel scan with progress, tombstone-aware error surfaces, opt-in `--fetch`, real-`git2` acceptance tests.
- Extend `crates/repograph-core/src/git.rs` with a `RepoStatus` model and `inspect()` adapter that produces it. Keep `git2` imports out of the binary crate.
- Establish a JSON envelope shape (`{ "repos": [ { ..., "state": "...", "error": null|"..." }, ... ] }`) stable enough that Phase 4 (`context`) can inline status without renegotiating the contract.
- Surface per-repo failures (missing path, broken git dir, fetch failure) as structured `error` fields plus stderr warnings, without aborting the batch — unless the user explicitly named a single broken repo positionally (then exit `3`).
- Preserve every behavior `registry-core` and `workspace-support` already ship; their archived tests must remain green.

**Non-Goals:**

- A `--porcelain` / `--short` output flag mirroring `git status --porcelain=v2`. Our JSON envelope is the machine-readable contract.
- `--ignored`, `--untracked-files=all`, submodule recursion, per-file diff output. These are richer-than-summary surfaces that belong (if anywhere) to a future `repograph diff` command.
- `--jobs <N>` concurrency tuning. `rayon`'s default global pool is correct for this workload; tuning flags are a future optimization, not a Phase 3 commitment.
- Caching status to disk between invocations. Status is point-in-time; caching invites staleness bugs.
- `git fetch --all` across all configured remotes. `--fetch` targets only the upstream of the current branch, per the principle of least network surprise.
- Effects on `repograph context` (Phase 4) and `repograph doctor` (Phase 5). `context` will consume the same `RepoStatus` type from `repograph-core`; that's an addition to Phase 4, not Phase 3.
- A `--watch` mode that re-runs on filesystem changes.
- Changes to `registry-core` or `workspace-support` specs. Both stay archived and invariant.

## Decisions

### Decision 1 — `RepoStatus` shape: structured fields, coarse `state` enum, optional `error`

**Choice:** A single `RepoStatus` struct with these fields:

```rust
pub struct RepoStatus {
    pub name: String,
    pub path: PathBuf,
    pub branch: Option<String>,    // None on detached/unborn/bare/missing
    pub upstream: Option<String>,  // None when no tracking branch is set
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    pub state: RepoState,
    pub error: Option<String>,
}

pub enum RepoState { Clean, Dirty, Detached, Unborn, Bare, Missing }
```

Serialized via `serde` with `#[serde(rename_all = "lowercase")]` on `RepoState`. `error` is `Some("...")` only for `Missing` / `Bare` / fetch failures; otherwise `None` (serialized as `null`, never omitted).

**Why:** A coarse enum makes the common UI affordances trivial (color the row red if `state != Clean`, show a warning icon if `error.is_some()`). Discrete counters (`staged`/`unstaged`/`untracked`) let downstream consumers reason about *which* kind of dirtiness without re-running `git status`. Keeping `error` always-present (never omitted, just `null`) matches the `dangling: []` precedent from Phase 2 — agents write `repo.error != null` without a key-existence check.

**Alternatives considered:**

- *Mirror `git status --porcelain=v2` verbatim.* Maps poorly to JSON; richer than needed; couples our contract to a `git` CLI format that has evolved twice already.
- *Bag-of-strings ("modified": ["a.txt", "b.txt"])*. Too verbose for `status`; the right home is a future `repograph diff` command.
- *Single `summary: String`.* Encodes display in the data, locks out machine readers. Rejected.

### Decision 2 — Parallelism via `rayon`'s global pool, no `--jobs` flag

**Choice:** Use `rayon`'s default global thread pool with `par_iter()` over the resolved repo list. No `--jobs <N>` flag in Phase 3.

**Why:** Repo-status work is mostly I/O-bound on disk reads inside `git2`. `rayon`'s work-stealing scheduler handles 5–500 repos well without tuning. Adding `--jobs` is non-breaking later; not adding it now keeps the surface small and avoids users picking the wrong value. The single integer choice is also bait for premature optimization — most users will not benefit from changing the default, and the few who do can wait for evidence-driven defaults in a later change.

**Alternatives considered:**

- *`tokio` async with `git2`.* `git2` is sync FFI on `libgit2`; bridging it through `tokio::task::spawn_blocking` adds dispatch overhead with no real-world latency benefit at our N. Rejected.
- *Manual `std::thread::spawn` loop.* Reinvents what `rayon` already does correctly. Rejected.
- *Sequential by default.* Defensible for small N, but workspace-X users routinely have 10+ repos. Sequential becomes the visible bottleneck of the command. Rejected.

### Decision 3 — Tombstone-aware failure: per-repo `state: missing`, batch continues

**Choice:** A registered repo whose path no longer exists, no longer opens as a git repo, or fails permission check does not abort the batch. It surfaces as `state: "missing"` (or `"bare"` for bare repos) with `error: "<message>"`. Exit code is `0` for batch invocations (all-repos default and `--workspace`). It is `3` if the user *explicitly named* a single broken repo positionally (`repograph status broken-repo`).

**Why:** Phase 2 already established the "drift is data, not failure" principle with the `dangling: []` field. Phase 3 extends it to drift in the *filesystem* layer: deleting `~/code/api` should not break `repograph status` for the other 29 repos. Surfacing the drift in the per-row `error` field (and a stderr `warn!` per missing repo) gives both humans and agents the information they need to act, without making the batch unusable.

The single-explicit-name case is different — the user asked specifically about `broken-repo`, and "the path is gone" *is* the answer; treating that as a successful batch with one broken row would hide the failure. Exit `3` is the same code Phase 1 uses for `add`/`remove` not-found cases, keeping the contract consistent.

**Alternatives considered:**

- *Abort the whole batch on any per-repo failure.* Punishes the common case; one stale registration shouldn't poison `status --workspace acme`. Rejected.
- *Skip silently.* Loses observability for both human and agent consumers. Drift accumulates invisibly. Rejected.
- *Exit `1` (general failure) on any per-repo error, even in batch mode.* Conflates two distinct semantics (batch with partial drift vs. command outright failed). The current rule — batch always `0`, single-name explicit `3` — is unambiguous.

### Decision 4 — `--fetch` is opt-in, bounded to current-branch upstream, per-repo failure isolated

**Choice:** No fetch by default. `--fetch` enables a `git2::Remote::fetch` of *only the upstream of the current branch* (not all remotes, not all branches). A fetch failure on a given repo populates that repo's `error` field with the fetch error message, sets `ahead`/`behind` from the pre-fetch state (or `0/0` if there was no prior tracking info), and continues the batch. Repos in `detached` / `unborn` / `bare` / `missing` state skip the fetch step entirely.

**Why:** Three principles:

1. **Zero-network default.** A `status` command that quietly hits the network on every run is hostile to offline workflows, CI sandboxes, and credential-walled remotes. Opt-in `--fetch` makes the network surface explicit.
2. **Minimal blast radius when enabled.** Fetching all remotes / all branches is what `git fetch --all` already does; users who want that have the tool. Our `--fetch` is targeted at the one question status needs: "where am I against the upstream I'm tracking right now?"
3. **Failure isolation matches Decision 3.** A 403 from one repo's remote shouldn't blank the ahead/behind for the other 29.

**Alternatives considered:**

- *Always fetch.* See above — hostile to offline use, surprising network calls.
- *Fetch all remotes.* Out of scope for what `status` needs; better surface for a future `repograph fetch` command.
- *Fetch concurrently *and* fail the batch on first error.* Loses isolation and makes `--fetch` brittle in exactly the cases where users would most want it (a workspace where one repo's auth is stale).

### Decision 5 — Names XOR workspace, no per-name dedupe surprise

**Choice:** `repograph status [<names>...] [--workspace <name>]` requires that positional names and `--workspace` are mutually exclusive. Passing both is a usage error (exit `2`). When neither is provided, scope is "all registered repos" (alphabetical, stable order).

If the user passes duplicate names (`repograph status foo foo`), the duplicates are collapsed silently before the scan — `status` is idempotent on its input set. Unknown names cause exit `3` and name the missing entry on stderr (same as `repograph remove`).

**Why:** Coexisting `names + --workspace` is ambiguous: "intersection?" "union?" "names override?" None of those are obviously correct, and each has surprise modes. Forbidding the combination at parse time avoids the ambiguity entirely; users who want both can run two commands. Dedupe is a quality-of-life choice that mirrors set semantics — repos are uniquely identified by name in the registry, and asking about the same repo twice should produce one row, not two.

**Alternatives considered:**

- *Allow both, treat as union.* Surprising for users who'd intuit intersection. Rejected.
- *Allow both, treat as filter.* "Names filtered to those also in the workspace" — defensible but a hidden join the JSON can't express in its shape. Rejected.
- *Error on duplicate names.* Punishes scripting; duplicates are rarely intentional but never destructive. Rejected.

### Decision 6 — `indicatif::MultiProgress` on stderr, cleared before stdout write

**Choice:** When stdout is a TTY (and only then), spawn an `indicatif::MultiProgress` on stderr with one spinner per repo, each labeled with the repo name. Spinners tick during the parallel scan. Before the renderer writes a single byte to stdout, the `MultiProgress` is dropped (which clears all spinners). When stdout is not a TTY (`--json`, piped, file-redirected), no spinners are drawn — only structured `tracing` log lines.

**Why:** The output contract is a hard guarantee: stdout is data, stderr is everything else. `indicatif` already writes to stderr, but spinners that overlap with stdout writes cause visible terminal corruption. Dropping the `MultiProgress` before the render call ensures we never interleave. The TTY gate prevents non-interactive uses (`status --json | jq`, status piped to a file) from getting either visual noise or stripped escape sequences.

**Alternatives considered:**

- *Single global spinner ("scanning 12 repos…").* Loses per-repo visibility; on a slow repo, the user can't tell whether the command is stuck or just waiting on that one. Rejected.
- *No progress UI at all.* `status` on 30 repos with a cold disk cache can take seconds. Silence is worse than a visible progress surface. Rejected.
- *Progress on stdout when not piped.* Violates the output contract. Rejected.

### Decision 7 — `--workspace` filter reuses `Config::resolve_workspace`, dangling silently skipped

**Choice:** `status --workspace <name>` calls `Config::resolve_workspace(name)` (the existing Phase 2 helper) to produce `(live, dangling)`. Status is computed only for `live` members; dangling names are not surfaced by `status` (parity with `list --workspace`, not with `workspace show`). Unknown workspace → exit `3`.

**Why:** Phase 2's design pinned the contract: `workspace show` is the canonical surface for *dangling* state; `list` is the canonical surface for *live repos*. `status` is in the same family as `list` (it answers questions about live repos), so it adopts the same skip-dangling rule. This keeps the two `--workspace` surfaces consistent and avoids two slightly-different definitions of "the repos in workspace X".

**Alternatives considered:**

- *Surface dangling in `status`'s JSON too.* Would duplicate `workspace show`'s contract under a different command, and the row layout has no natural home for tombstones (a tombstone is just a name; there's no path, no branch, no state to report). Rejected.
- *Error on any dangling member in the filtered workspace.* Over-strict; mirrors Cargo's behavior, which Phase 2 already rejected. Rejected.

### Decision 8 — JSON envelope reuses `{ "repos": [...] }` shape

**Choice:** Status's JSON envelope is `{ "repos": [ <RepoStatus>, ... ] }` — same outer key as `registry-core`'s `list`. The inner objects are richer (they carry status fields), but the wrapper is identical.

**Why:** Agent consumers should not have to switch parsers between `list --json` and `status --json`. A single envelope key + a per-entry schema that's a strict superset of `list`'s schema means generic tooling (jq filters, downstream JSON-RPC bindings, MCP wrappers) can compose. Phase 4's `context` envelope can extend the same pattern.

**Alternatives considered:**

- *Wrap as `{ "statuses": [...] }`.* Cleaner-sounding but breaks composability with `list`. Rejected.
- *Top-level array `[ <RepoStatus>, ... ]`.* Loses the envelope discipline `registry-core` established; future fields (counts, summaries) would force a breaking schema change. Rejected.

## Risks / Trade-offs

- **[Long-running `git2` calls on a single huge repo could stall a batch behind one slow row]** → Mitigation: `rayon`'s work-stealing pool naturally proceeds on other repos while one is slow; the spinner remains visible on stderr so the user knows which repo is taking time. We do not add a per-repo timeout in Phase 3 (that's a future optimization with its own correctness tradeoffs around abandoning in-flight `libgit2` operations).
- **[`--fetch` blows up if user has stale credentials on one of many repos]** → Mitigation: per-repo isolation (Decision 4). The bad repo populates its `error` field; the rest proceed.
- **[`rayon` global pool + `tracing` spans can produce interleaved log lines]** → Mitigation: log lines carry the bound repo name via `#[tracing::instrument(fields(repo = %name))]`; even when interleaved, each line is self-describing. The stderr-only output contract means this is purely a diagnostic concern, not a data-contract concern.
- **[`indicatif` spinners corrupt the terminal when an error message logs mid-scan]** → Mitigation: route `tracing` writes through `tracing-indicatif` (or, if that complicates the dependency tree, manually suspend the MultiProgress around the log call). Either way, spinners are cleared before stdout writes, so a corrupted stderr does not contaminate the data contract.
- **[`state: detached` + `--fetch` is a no-op surprise for users]** → Mitigation: documented in `--help` and README. The JSON `error` field on a detached repo is `null` (no failure happened); the `branch` field is `None` with the short SHA exposed in stderr's `warn!` for context.
- **[Adding `rayon` as a dependency increases compile time for the binary crate]** → Accepted. `rayon` is small and widely used; the binary already pulls in `indicatif` and `git2`, both heavier. Compile-time impact in `cargo install` users is negligible.
- **[Future Phase 4 `context` will want to embed `RepoStatus`; today's envelope must not paint Phase 4 into a corner]** → Mitigation: `RepoStatus` lives in `repograph-core`, fully `serde`-serializable, with no binary-crate-specific concerns. Phase 4's envelope can compose it directly. The `error: null` precedent generalizes — Phase 4's per-repo entry can carry both `status` and `context` fields with the same null-not-omitted discipline.
- **[Stale `ahead`/`behind` when the user has not fetched recently]** → Accepted and documented. Without `--fetch`, the numbers reflect the last `git fetch` (manual, IDE-driven, or otherwise) the user ran. The TTY rendering and the `--help` text both note "ahead/behind relative to the last fetched upstream state". Users who need fresh numbers can run with `--fetch`.

## Migration Plan

There is no on-disk migration. `status` is a read-only consumer; the TOML config schema is untouched, and the workspace-support and registry-core specs are not modified. Existing users running `cargo install repograph` after the Phase 3 release pick up the new command with zero action required.

No version field, no migration script. The JSON envelope is additive: `list --json` consumers see no change; new `status --json` consumers see a strict-superset schema.

For users running the prior `repograph` build: zero action required.

## Resolved deviations

- **Decision 5 enforcement layer.** The original plan (and tasks.md 3.3) was to enforce the names-XOR-workspace rule at runtime via a `RepographError::UsageError` returned from `commands::status::run`. Switched to `clap`'s `#[arg(conflicts_with = "names")]` on the `--workspace` argument so the rejection happens at parse time. Reason: `UsageError` maps to exit `1`, but the spec mandates exit `2` for this case; clap's native conflict handling exits `2` and emits the standard "argument cannot be used with..." stderr message, matching every other argument error in the binary. No behavior visible to spec scenarios changes — the acceptance test for `names + --workspace` exits `2` exactly as the spec requires.

- **`git2` build features.** The pre-existing `git2` dependency in `repograph-core/Cargo.toml` was pinned with `default-features = false, features = ["vendored-libgit2"]` — which compiles `libgit2` without `https` or `ssh` transports. The Phase 1 and 2 commands never touched the network, so this only surfaced when smoke-testing `repograph status --fetch` against a real GitHub remote: every fetch produced `error: "unsupported URL protocol"` instead of a real network call. Added `vendored-openssl`, `https`, and `ssh` to the feature list. `vendored-openssl` is required for cargo-dist's cross-platform binaries to work without depending on the host's OpenSSL ABI. Two acceptance tests (`fetch_supports_https_transport`, `fetch_supports_ssh_transport`) pin the transports by pointing at a guaranteed-closed port and asserting the resulting error is *not* the transport-missing signature. Cost: ~4 MB of additional binary size from the vendored OpenSSL + libssh2 (bundled inside libgit2) and a one-time compile-time hit. Net: `--fetch` works against the URL forms every real user has.

- **Credential callbacks for authenticated remotes.** The initial `run_fetch` helper called `Remote::fetch(&[branch], None, None)` with no `FetchOptions` and therefore no credential callbacks. Once the `https`/`ssh` transports were available, the next smoke-test failure surfaced as `error: "authentication required but no callback set"` — libgit2 reached the auth challenge but had no way to obtain credentials. Wired a `RemoteCallbacks::credentials` closure that mirrors `git fetch`'s default behavior: try `Cred::ssh_key_from_agent` for SSH challenges (using the URL's username, defaulting to `git`), `Cred::credential_helper` against `Config::open_default()` for HTTPS basic-auth challenges (honors the user's `credential.helper` config — Keychain on macOS, libsecret on Linux, Windows Credential Manager, etc.), and `Cred::default()` as a final fallback. Each branch is one-shot per fetch via a captured boolean so libgit2 can't loop a failing method. No new prompts, no new stored secrets — auth surface is exactly the user's existing git/SSH config. The acceptance tests assert the resulting error never contains "no callback set" so regressions surface immediately. Out of scope: SSH key-from-file with passphrase prompts, custom auth helpers, two-factor flows — all are downstream of a future Phase that introduces an interactive surface.

## Open Questions

None at proposal time. All design questions identified during exploration (parallelism, fetch semantics, tombstone behavior, names-vs-workspace, JSON envelope shape) are resolved above with industry prior art or explicit reasoning, and rejected alternatives are recorded so a future contributor can audit the trail.

Questions that may surface during implementation, captured here so they aren't lost:

- The exact `comfy-table` column ordering when both `name` and `branch` are very long (truncation strategy: ellipsis-on-the-right vs. wrap). Cosmetic; tasks will pin it.
- Whether to expose a coarse `state: "ahead"` / `state: "behind"` variant when `ahead > 0` or `behind > 0` and the working tree is clean. Tempting but it overloads the enum — leaving `state: Clean` and letting consumers read `ahead`/`behind` fields keeps the enum precise. Re-evaluate only if Phase 4 needs a different signal.
- Whether `--fetch` should also be available as `REPOGRAPH_STATUS_FETCH=1` env var for CI users. Defer — flag-first; env var only if a real ask appears.
- The default `RepoState` for a registered repo whose path is a *directory* but not a git repo (was a repo at registration time, `.git` was deleted). Current plan: `state: "missing"` with `error: "no longer a git repository"`. Alternative `state: "broken"` was considered; collapsing into `Missing` keeps the enum small and avoids splitting hairs over what counts as "missing" vs. "broken".
