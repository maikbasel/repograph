## Context

Phase 5 — Shell & Polish — is the last planned change in the dev plan. After it, repograph is feature-complete for v1: every subcommand from the original surface ships, every doc target is hit, every health-check the binary needs to debug user reports is in place. The change name `shell-integration` is the umbrella, but it covers two distinct user-facing capabilities (`shell-integration` itself for `switch` + `completions`, and `doctor-command` for the diagnostic surface) that share the same architectural seams.

**Current state**:

- `Config::repos()`, `Config::repo(name)`, `Config::workspaces()`, `Config::workspace(name)`, and `Workspace::resolve(&Config)` (returning `(live, dangling)`) are all stable from `registry-core` and `workspace-support`.
- `repograph_core::git::validate_git_repo(path)` already opens a `git2::Repository` and returns the appropriate `RepographError` on failure — exactly the building block `doctor`'s per-repo check needs.
- `repograph_core::context::resolve_agent_docs(repo_root, agents)` already walks the agent registry's patterns and returns `Vec<AgentDoc>` with per-file warnings — exactly the building block `doctor`'s agent-doc presence check needs.
- `RepographError::NotFound { kind, name }` already maps to exit `3` — `switch` reuses it without adding a variant.
- The `Cli` parser in `crates/repograph/src/main.rs` is a `clap::Parser`-derived struct; `clap_complete::generate` needs `<Cli as clap::CommandFactory>::command()` to introspect it — no refactor needed, just a one-line helper.
- `output::with_progress` (from `context-command`) already wraps `rayon` per-repo fan-out with TTY-aware `indicatif::MultiProgress` spinners — `doctor` reuses it.
- The output contract (stdout = data, stderr = diagnostics) is settled and applies to all three new commands, with one important twist: for `switch`, the stdout payload is itself a shell command, not "data" in the JSON/table sense. The contract still holds — stderr stays diagnostic — but the JSON branch is intentionally absent.

**Constraints (from CLAUDE.md and `.claude/rules/`)**:

- No `unwrap`/`expect` outside test code; every failure mode maps to a `RepographError` variant with a documented exit code.
- `git2` only — `doctor`'s per-repo git-repo validity check goes through `repograph_core::git::validate_git_repo`, never shells to `git`.
- Tests use `tempdir` + real `git2`; no mocks. Acceptance tests via `assert_cmd` drive the design top-down.
- `tracing` for diagnostics; stdout reserved for payload (or, for `switch`, the bare `cd` line).
- No `todo!()`, no half-finished implementations — both output modes ship for `doctor`; both are tested; every check in the catalog is wired and tested.

**Stakeholders**:

- TTY users running `rg-cd <name>` (or `repograph switch <name> | eval`) hundreds of times per day — `switch` has to be fast (sub-10ms) and the stdout has to be unconditionally shell-safe.
- TTY users running `repograph completions <shell>` once at install time — `completions` has to produce output that the shell's completion loader accepts without modification.
- TTY users debugging "why is my context payload empty?" — `doctor` has to surface the cause (missing repo path, dangling workspace member, agent doc not present) clearly enough that the user knows what to fix.
- CI / agents running `repograph doctor --json` as a health gate — the JSON envelope has to be stable and `schema_version`-versioned from day one, same contract as `context-command`.
- The future `repograph-mcp` server, which will likely expose `DoctorReport::run(&Config)` as an MCP tool — so the core API surface needs to be reusable, not CLI-shaped.

## Goals / Non-Goals

**Goals:**

- Produce one canonical `cd <path>` line for `switch <name>` that any POSIX shell, fish, or PowerShell can `eval` without escaping — exact stdout shape is the contract.
- Generate completions for every shell `clap_complete::Shell` enumerates (`bash`, `zsh`, `fish`, `powershell`, `elvish`) by introspecting the live `Cli` struct — completions can never drift from the actual command surface because they're regenerated against it.
- Aggregate every check repograph can perform against its own config into one `DoctorReport`, render it as a TTY-friendly summary table or a `schema_version: 1` JSON envelope, and exit with a code that lets CI gate on it (`0` on clean / warn-only, `1` on any error finding).
- Reuse `validate_git_repo` and `resolve_agent_docs` from core — no duplication of git-open or agent-pattern logic.
- Document the shell snippets and completion install one-liners in `README.md` so users who pull `repograph` from a release artifact can wire it into their shell in under a minute.
- Stay within the existing exit-code contract — no new codes added.

**Non-Goals:**

- **A `switch --shell <bash|fish|powershell>` flag.** Shells parse `cd <path>\n` identically (modulo path quoting, which we handle by emitting a single-line `cd '<path>'` form when the path contains whitespace or shell metacharacters). One stdout shape covers everything. If a future shell breaks this, we'll add a `--shell` flag then.
- **A `switch --print` flag that emits the bare path.** YAGNI — pipe through `cut -d' ' -f2-` if you really need it; the binary's job is to emit the eval-ready line.
- **Companion `repograph cd` subcommand that changes the parent shell's cwd.** Not possible — child processes can't `chdir` the parent. The `rg-cd` shell function on the user side is the intended UX.
- **Installing completions automatically.** The `completions` subcommand writes to stdout; the user redirects to the path their shell expects. Auto-installation would require knowing the user's shell layout and is fragile across distros.
- **A `completions install` mode.** Same reason — installation is per-shell-per-distro. Documenting the one-liner in README is enough.
- **`doctor --fix` / interactive remediation.** `doctor` is read-only; it surfaces drift but never mutates config. Remediation lives in `repograph remove`, `repograph workspace remove`, and `repograph init` (re-register, re-prompt). Mixing the two would muddy the contract.
- **`doctor` runs `git fetch` / network calls.** Zero-network by construction, same as `repograph status` without `--fetch`.
- **`doctor --workspace <name>` / `doctor <repo>...` scope flags.** The check catalog is whole-config; per-repo scoping is a YAGNI shape we'd revisit if the report ever gets too long to skim.
- **Per-check severity overrides via flags or config.** Severities are intrinsic to each check (a missing path is always `error`; a missing optional agent doc is always `warn`). Tunable severity invites cargo-culting and would obscure the contract.
- **A `doctor --strict` flag that escalates warnings to errors.** Same concern. If a downstream consumer wants strictness, they parse the JSON envelope and gate on `summary.warn > 0`.

## Decisions

### Decision 1: `switch` stdout is exactly `cd <path>\n` — no JSON, no banner, no log lines

**Choice**: `repograph switch <name>` writes a single line to stdout: `cd ` followed by the registered repo's canonical absolute path, terminated by a single `\n`. No other bytes ever reach stdout. Paths that contain whitespace or any shell metacharacter (the set `[ \t\n'"$\\`*?[\]{}();&|<>!#~]` is conservative) are wrapped in single quotes with embedded single quotes escaped as `'\''` (the canonical POSIX `printf %q`-equivalent for single-quoted strings). Paths without metacharacters are emitted unquoted for readability.

**Rationale**:

- The line is meant for `eval` in a shell function — extra bytes break the eval. A trailing newline is what `eval` and the shell's command parser expect.
- Quoting only when needed keeps the output readable on the 99% case while staying safe on the 1% (paths with spaces on macOS, Windows-style absolute paths under WSL).
- No `--json` mode because JSON output would break the eval contract. The command is so specialized that the existing TTY-vs-non-TTY branch doesn't apply — stdout is always shell-eval-safe, regardless of where it's pointed.

**Alternatives considered**:

- **Always single-quote**: simpler implementation, but uglier in the common case and noisier in logs.
- **Always `printf '%q'`-style POSIX quoting via a crate**: an extra dep for a 30-line problem. The set of metacharacters we care about is small and well-known.
- **Emit `cd "$ARGV"` and let the shell expand**: shifts the burden to the user's shell function, which is exactly what we don't want. We own the contract.

### Decision 2: `switch` exit `3` on unknown name with "did you mean ..." suggestion

**Choice**: When `<name>` does not resolve, exit `3` (the existing not-found code), reuse `RepographError::NotFound { kind: "repo", name }`. Before raising, compute the Levenshtein distance from `<name>` to every registered repo name; if any are within distance `2` (and within `0.5 * len(name)` to suppress dumb suggestions on very short names), append a "did you mean: a, b, c?" line to stderr.

**Rationale**:

- `switch` is the command users type fastest and most often; typo recovery is high-value UX.
- Distance ≤ 2 and the half-length guard match the conventions used by `cargo`, `npm`, and `git` for their own "did you mean" output.
- The suggestion goes on stderr (the diagnostic channel), not stdout — stdout stays clean of any non-`cd` output even on the error path. Suggestions are absent (silent) when no candidates pass the threshold.

**Alternatives considered**:

- **No suggestions**: leaves a clear UX gap; users retyping with `repograph list` is friction.
- **Top-3 closest by raw distance with no threshold**: noisy on small registries; suggests `ui` when the user typed `lib` just because the registry has nothing closer.
- **Pull in `strsim`**: a 50-line Levenshtein in `commands/switch.rs` is enough; no need for the dep. Acceptable trade-off — small code, no transitive deps.

### Decision 3: `completions` introspects the live `Cli` via `clap::CommandFactory`

**Choice**: `commands/completions.rs` calls `<Cli as clap::CommandFactory>::command()` to obtain the `clap::Command` AST and passes it to `clap_complete::generate(shell, &mut cmd, "repograph", &mut io::stdout())`. The `Cli` type is declared `pub(crate)` (the rest of `main.rs` doesn't need it public; the completions module imports it via `super::Cli`).

**Rationale**:

- Single source of truth: the `Cli` struct defined in `main.rs` is the one clap parses against at runtime; introspecting it for completions guarantees the completion script can never list a flag that doesn't exist or miss one that does.
- `clap_complete` is the upstream-blessed approach; no need to hand-write per-shell scripts.
- `clap_complete::Shell` is the enum we accept as a positional arg; clap rejects unknown values with exit `2` automatically.

**Alternatives considered**:

- **Hand-written completion scripts checked into `completions/`**: drifts the moment any subcommand or flag changes. Rejected.
- **A `build.rs` that emits completions at compile time and bundles them as installer artifacts**: nice-to-have for cargo-dist users, but orthogonal to having the `completions` subcommand for `cargo install` / source-build users. We pick the simpler, always-available path; the cargo-dist enhancement is YAGNI for v1.
- **`clap_mangen` for man pages too**: out of scope for this change. The man-page surface deserves its own beat; bundling it here bloats the change.

### Decision 4: `doctor` check catalog — fixed for v1, owned by `repograph-core::doctor`

**Choice**: The check catalog is a closed enum `Check` in `repograph_core::doctor`. v1 covers:

| Check                       | What it verifies                                                                                | Severity on fail |
|-----------------------------|-------------------------------------------------------------------------------------------------|------------------|
| `ConfigPresent`             | Config file exists at the resolved config dir                                                   | `error`          |
| `ConfigParse`               | Config file parses as TOML (only run if `ConfigPresent` passed)                                 | `error`          |
| `AgentsConfigured`          | `[agents]` section is present in config                                                         | `warn`           |
| `ProjectsRootExists`        | `[settings].projects_root`, if set, points at a real directory                                  | `warn`           |
| `RepoPathExists`            | Per repo: the registered path exists on disk                                                    | `error`          |
| `RepoIsGitRepo`             | Per repo: the path opens as a `git2::Repository` (only run if `RepoPathExists` passed)          | `error`          |
| `WorkspaceMembersResolve`   | Per workspace: every `members[*]` name is a registered repo name (dangling members are reported)| `warn`           |
| `AgentDocPresent`           | Per repo × per selected agent: at least one file matches the agent's pattern set                | `warn`           |

Each finding carries `{ check: Check, severity: Severity, target: String, message: String }`. `target` is `"<repo-name>"`, `"<workspace-name>"`, `"<repo-name> / <agent-id>"`, or `"<config-file-path>"` depending on the check — opaque to the renderer, useful as a stable sort key.

**Rationale**:

- A closed enum makes the catalog the contract. New checks land as enum variants with a migration moment (specs, README, JSON `Check` value addition).
- Severities are intrinsic per the Goals section (no overrides) — `error` if the check failing means the user's data is broken; `warn` if it means the user's setup is sub-optimal but still works.
- Splitting `RepoPathExists` from `RepoIsGitRepo` lets us emit two separate findings for two distinct user actions (re-register at the new path vs. re-init the repo). Conditional execution (`RepoIsGitRepo` only runs if `RepoPathExists` passed) avoids cascading noise.
- `AgentDocPresent` is `warn`, not `error`: a repo with `[agents].selected = ["claude-code", "cursor"]` but no `CLAUDE.md` in repo X is a useful signal (the `context` payload's `cursor` section will be empty) but not a broken state — the user may have just not written one yet.

**Alternatives considered**:

- **An open trait `trait Check { fn run(&self, &Config) -> Vec<Finding>; }`**: more extensible, but YAGNI for v1 and harder to round-trip in JSON (the `check` field becomes opaque). Reserve as v2 if we accumulate enough checks.
- **A free-form `Vec<String>` of finding messages**: kills the JSON contract. Rejected immediately.
- **Bundling `RepoPathExists` and `RepoIsGitRepo` into one `RepoHealthy` check**: loses the action-distinction granularity. Rejected.

### Decision 5: `doctor` JSON envelope — versioned, same shape as `context-command`

**Choice**:

```json
{
  "schema_version": 1,
  "generated_at": "2026-05-24T14:23:11Z",
  "checks": [
    {
      "check": "RepoPathExists",
      "severity": "error",
      "target": "api",
      "message": "path /home/user/code/api does not exist"
    },
    {
      "check": "AgentDocPresent",
      "severity": "warn",
      "target": "ui / claude-code",
      "message": "no files matched CLAUDE.md"
    }
  ],
  "summary": { "ok": 12, "warn": 1, "error": 1, "total": 14 }
}
```

- `schema_version` is the integer `1` from v1; additive-only changes at version `1`; breaking changes bump the version. Same policy as `context-command`.
- `checks` is sorted by `(severity DESC, check ASC, target ASC)` so the most pressing items appear first in both JSON and table output.
- `summary.total` is `ok + warn + error`. `ok` counts findings explicitly emitted with severity `ok` (e.g. per-repo `RepoPathExists` passes are reported as `ok` findings so consumers can audit what was checked, not just what failed).
- The JSON output is a single-line emission (jq-friendly), same as `context-command`.

**Rationale**:

- Reusing the `schema_version` + sorted-array + `summary` pattern from `context-command` keeps the JSON dialect across the binary internally consistent — downstream agents only have to learn one shape.
- Emitting `ok` findings (not just failures) lets CI auditors verify the check ran at all on a given config; "no findings" is ambiguous otherwise (did everything pass, or were no checks executed?).
- Stable sort lets diff-based monitoring of `doctor --json` output between runs flag drift as a real change, not a reordering artifact.

**Alternatives considered**:

- **Suppress `ok` findings from JSON output**: smaller payload, but makes "did this check run?" un-answerable without consulting the catalog separately. Rejected for v1.
- **A `findings_by_severity` map keyed by severity**: nicer-looking, but loses stable iteration order and complicates the schema. Rejected.
- **Embed the check catalog version separately from `schema_version`**: over-engineered for v1. The catalog and envelope move together at v1.

### Decision 6: `doctor` TTY rendering — a `comfy-table` summary plus a count footer

**Choice**: TTY mode renders:

1. A `comfy-table` preset `UTF8_FULL` (same as other commands) with columns `Severity | Check | Target | Message`. Rows sorted as in JSON. Severity column is colorized via `console::Style` (`error` red, `warn` yellow, `ok` green).
2. A single footer line: `12 ok · 1 warn · 1 error` (with the same color treatment per count).
3. When `summary.error > 0`, an additional `→ run \`repograph doctor --json | jq\` for machine-readable detail` hint on stderr.

**Rationale**:

- Consistent with `list`, `status`, and `workspace ls` — same preset, same column conventions, same `console` color application.
- Colors land only on a TTY (`console::Style` no-ops when stdout isn't a TTY); the table renders fine in non-TTY too, though `--json` is the recommended non-TTY shape.
- The trailing hint on stderr (not stdout) keeps the table contract clean while pointing users at the structured output.

**Alternatives considered**:

- **Grouping the table by severity with headers**: prettier but breaks the "single tabular data shape" convention. Rejected.
- **A per-repo nested layout (tree view)**: nicer for big configs, but `comfy-table` doesn't render trees and adding a tree renderer is out of scope.

### Decision 7: `doctor` parallelism — `rayon` per repo, sequential per workspace and per check

**Choice**: The per-repo checks (`RepoPathExists`, `RepoIsGitRepo`, `AgentDocPresent` × N agents) fan out across the registered repo list via the existing `output::with_progress` helper (which already wraps `rayon::par_iter` with `indicatif` TTY spinners). Workspace and config-level checks run sequentially on the main thread (cheap, no benefit from parallelism). Findings collect into a `Vec<Finding>` which is then sorted post-collection — same shape as `context-command`'s `RepoContext` aggregation.

**Rationale**:

- File I/O per repo dominates; serial check execution is wasteful on large registries.
- Reusing `output::with_progress` gets the TTY spinner UX for free and stays consistent with `status` and `context`.
- Sorting post-collection guarantees stable output regardless of `rayon`'s scheduling.

**Alternatives considered**:

- **Fully sequential**: simpler, slower. Rejected — we already paid the cost of wiring up parallel I/O for `context`; reusing it is essentially free.
- **Per-check fan-out (one task per check across all repos)**: more granular but harder to reason about. Per-repo fan-out matches the existing pattern and is good enough.

### Decision 8: `doctor` exit codes — `0` on no-error, `1` on any error, `4` on permission-denied reading config

**Choice**:

- `0` — every finding has severity `ok` or `warn`. Warnings do not gate.
- `1` — any finding has severity `error` (including the synthetic "config not found" finding).
- `4` — config file exists but cannot be read due to permission denied. The report is not produced; the error surfaces through the standard `RepographError::Io` path. No JSON envelope is emitted in this case (stdout is empty).

**Rationale**:

- `1` is the catch-all for "the binary completed but something is wrong with your state"; CI gates can use it as the signal to fail.
- `4` for permission-denied stays consistent with the contract (`RepographError::Io` → `4` when `ErrorKind::PermissionDenied`). The user's recourse is to fix file permissions, not to look at a partial doctor report.
- `0` on warn-only avoids alarm fatigue — `repograph doctor` should be runnable from `precmd` / shell prompt hooks without breaking the prompt on a single missing optional agent doc.

**Alternatives considered**:

- **`1` on any non-`ok`, including warnings**: too aggressive; turns `doctor` into a chore.
- **A `--strict` flag escalating warnings to errors**: see Non-Goals.
- **Distinct codes per check category**: over-fits the contract; no consumer asked for it.

### Decision 9: `switch` and `completions` need no `git2`, no config write — keep them O(1)

**Choice**: `switch` loads the config (TOML deserialize is fast, ~ms on real configs), looks up the repo by name (single `HashMap`/`BTreeMap` get), formats the line, writes it. No `git2::Repository::open`, no validation of "is the path still a git repo" (that's `doctor`'s job — `switch` trusts the registry). `completions` doesn't even load the config; it introspects `Cli` and writes the script.

**Rationale**:

- `switch` is invoked through a shell function on every directory hop the user takes; latency matters.
- `git2::Repository::open` is comparatively expensive (libgit2 walks parent dirs for a `.git/`, even when given a leaf path); avoiding it on the hot path is a measurable win.
- If the registered path is bogus, the shell's `cd` will fail with its own diagnostic — which is more useful than a repograph-side error because it preserves the user's expectation that errors come from the shell.

**Alternatives considered**:

- **Validate before printing**: trades latency for an arguably better error message (we'd emit `repograph: 'api' at /old/path no longer exists` instead of letting `cd` complain). Net negative — the user re-runs `repograph doctor` if they hit this, which is exactly what `doctor` is for.

### Decision 10: Module placement — `core` owns `doctor`, binary owns the renderers and the three CLI commands

**Choice**:

- New core module: `crates/repograph-core/src/doctor.rs` with `Finding`, `Severity`, `Check`, `DoctorReport`, and `DoctorReport::run(&Config, &Path)` (the second arg is the config-dir path, needed for the `ConfigPresent` check's `target` field). Pure function; no terminal I/O; no clap.
- New binary modules: `crates/repograph/src/commands/switch.rs`, `crates/repograph/src/commands/completions.rs`, `crates/repograph/src/commands/doctor.rs`. Each has `Args` (clap derive) and `run`. The `doctor` command delegates to `DoctorReport::run`, then routes to `output::render_doctor_{table,json}`.
- `output.rs` gains `render_doctor_table(report: &DoctorReport, w: impl Write)` and `render_doctor_json(report: &DoctorReport, w: impl Write)` — same naming convention as the existing context renderers.
- `commands/switch.rs` writes its `cd <path>` line directly via `writeln!` on `io::stdout()` — no detour through `output.rs` because the line isn't tabular data and going through a renderer would over-engineer a one-liner.

**Rationale**:

- Same boundary discipline as `context-command`: core owns the domain (the check catalog, the finding aggregation, the deterministic sort) and stays terminal-free; the binary owns clap, color, table rendering, and stdout/stderr discipline.
- A future `repograph-mcp` server gets `DoctorReport::run` as a reusable API without dragging in `comfy-table` or any TTY dep.
- `switch`'s in-place `writeln!` is justified by its uniqueness — one line of structured-but-not-tabular output that an additional renderer can't make better.

### Decision 11: Tracing — entry/success/error per command, with finding counts on `info` for `doctor`

**Choice**: Following `.claude/rules/logging.md`:

- **`switch`**: `debug!(command = "switch", name = %args.name, "start")` on entry; `info!(repo = %name, path = %path.display(), "resolved")` on success; `error!(err = ?e, name = %args.name, "switch failed")` on the error path.
- **`completions`**: `debug!(command = "completions", shell = ?args.shell, "start")` on entry; `info!(shell = ?args.shell, "completions generated")` on success; `error!(err = ?e, shell = ?args.shell, "completions failed")` on error.
- **`doctor`**: `debug!(command = "doctor", json = args.json, "start")` on entry; `info!(ok = report.summary.ok, warn = report.summary.warn, error = report.summary.error, "doctor complete")` on success; `error!(err = ?e, "doctor failed")` on error. Per-finding `warn!` lines for `Severity::Warn` and `error!` lines for `Severity::Error` are NOT emitted (they'd duplicate the table/JSON output and create stderr noise); the structured payload is the single source of truth.

**Rationale**: explicit in the logging rule; the auto-loaded contract.

## Risks / Trade-offs

- **[Risk] `clap_complete` pulls in transitive deps that bloat the binary** → Mitigation: `clap_complete` is already in clap's family; the on-disk size delta is small (verify with `cargo bloat` after adding). Acceptable cost for the feature.
- **[Risk] The `switch` quoting heuristic misses an exotic shell metacharacter and someone's path breaks** → Mitigation: the metacharacter set is conservative (errs toward over-quoting); single-quoted strings with `'\''` escaping are the most portable POSIX form. Add a test case for any new failure mode encountered in the wild.
- **[Risk] The Levenshtein "did you mean" suggestion produces dumb results on tiny registries** → Mitigation: the half-length guard suppresses suggestions when the typo is too short to meaningfully compare. Suggestions are stderr-only — they never break stdout's eval contract even if wrong.
- **[Risk] The `doctor` JSON envelope becomes unstable as we add checks** → Mitigation: `schema_version: 1` from day one; additive-only at v1 (new `Check` variants are additive on the JSON side because parsers see them as new strings to ignore-or-handle); breaking changes bump the version. Same policy as `context-command`.
- **[Risk] `doctor`'s per-repo `git2::Repository::open` is slow on a 50-repo registry on a cold cache** → Mitigation: fan out via `rayon` (Decision 7). If still slow, expose `--jobs N` later; not v1 scope.
- **[Risk] `AgentDocPresent` check duplicates work `context` does** → Mitigation: it calls the same `resolve_agent_docs` helper from `repograph_core::context`. The check only needs `agent_doc.files.is_empty()`, not the file contents — `resolve_agent_docs` already short-circuits on the existence check before reading file bodies, so the I/O cost is bounded by the pattern walk, not the file read.
- **[Trade-off] `switch` makes no validity guarantee about the path it emits** → The user's shell gets the diagnostic. `doctor` is the validity-check tool; `switch` is the teleport tool. Two tools, two responsibilities. Documented in README.
- **[Trade-off] Three new subcommands in one change** → Larger change footprint than typical, but they share the same plumbing (clap registration, `output.rs` extensions, README updates) and all three are required for Phase 5's "Shell & Polish" outcome. Splitting into three changes would triple the OpenSpec ceremony for what is, architecturally, one cohesive polish pass.
- **[Trade-off] `completions` doesn't auto-install** → Per-shell-per-distro autoinstall is fragile. The one-liner redirect in README is good enough; cargo-dist may eventually add tap-managed completions on its own schedule.
- **[Risk] Adding a `Cli` factory helper in `main.rs` couples the completions command to the binary's CLI shape** → That coupling is desired; the whole point of using `clap::CommandFactory` is to derive completions from the live shape. If the `Cli` type changes (new subcommand, new flag), completions regenerate against it automatically the next time the user runs the command.

## Migration Plan

No migration. Three new commands; no existing config is touched; no breaking changes to existing commands. The `[agents]` schema, `[settings]` schema, and `ensure_agents_configured` helper are all read-only consumers from `doctor`'s side.

**Rollback strategy**: revert the change set; no on-disk state to clean up. Completions scripts the user installed are static files — they keep working against the previous binary version (clap completions are forward-compatible with new subcommands; old completions simply don't list the new ones).

## Open Questions

_None at proposal time._ Open items will be tracked here as implementation surfaces them.

## Resolved deviations

Per `.claude/rules/documentation.md`, deviations from the original plan are recorded here rather than rewritten retroactively.

- **`doctor` runs checks sequentially in core, not via `rayon` per-repo fan-out.** Decision 7 specified parallel per-repo checks via the existing `output::with_progress` helper. The implementation runs the catalog sequentially inside `repograph_core::doctor::DoctorReport::run`, which keeps core free of `rayon` (consistent with the same trade-off in `context-command`'s Resolved deviations) and is fast enough for the v1 catalog: a 20-repo `doctor` call is comfortably sub-second on a warm cache because the per-repo work is just `Path::exists` + `git2::Repository::open` + a handful of `fs_err::metadata` calls per agent. Revisit if a real-world report ever blocks visibly; until then, sequential is the simpler, smaller-API choice.
- **`DoctorReport::run` lives in core; the binary stamps `generated_at`.** Matches Decision 10 / 11. The `generated_at` string is passed in from `commands/doctor.rs::now_rfc3339` (via the `time` crate already in the binary's deps) so core stays time-dep-free, identical to how `context-command` handles the same field.
- **New `RepographError::DoctorErrorsFound { count }` variant, mapped to exit `1`.** Decision 6 of the original tasks contemplated either a dedicated variant or reusing `UsageError`. We picked the dedicated variant for two reasons: (1) it lets `main::report` skip the generic `tracing::error!("repograph failed")` line on this specific variant, since the doctor report on stdout is already the user-facing surface and a trailing "repograph failed" line would be confusing noise; (2) it keeps the error type honest about *what* failed — semantically distinct from a usage error.
- **`switch` does not load `[agents]` or trigger first-run prompts.** Already in spec, but worth restating: `switch::run` calls `Config::load` directly without going through `ensure_agents_configured`. This keeps the hot path (every `rg-cd` invocation) at one TOML deserialize and one `HashMap::get`, sub-10ms in practice.
- **`switch` suggestion line goes through `tracing::error!`, not bare `eprintln!`.** Project logging rule says all diagnostic output flows through `tracing` (which is configured to write to stderr by `init_tracing` in `main.rs`). The acceptance test for the suggestion path captures stderr via `assert_cmd` and confirms the `did you mean: …` text lands there — the `tracing` formatter wraps it with timestamps and target prefixes, but the substring assertion `stderr.contains("did you mean")` still matches.
