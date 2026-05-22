## Context

`registry-core`, `workspace-support`, and `git-status` are archived. The remaining Phase 4 goal is "agent-facing context aggregator" — composing the registry, workspaces, and git state with inlined agent docs (CLAUDE.md / AGENTS.md / etc.). During exploration we concluded that *which* docs to inline must be a declared user choice, not a discovery rule baked into the codebase. That declaration belongs in an explicit setup step. `repograph init` becomes Phase 4a, unblocking the context aggregator (Phase 4b) cleanly.

The current registry model has no notion of agents or per-user toolchain preferences. Existing commands all follow the same shape — a clap `Args` struct, a `run(args, config_dir) -> Result<(), RepographError>` function, `tracing` instrumentation at command entry, TTY check via `OutputMode::detect(args.json)`, and rendering via helpers in `output.rs`. Init breaks the mold on two counts: it is the first interactive command (multi-step prompts, not just argument parsing), and it composes multiple existing primitives (`Config::add_repo`, `Config::create_workspace`, `Config::add_members`) inside a guided flow.

Stakeholders: solo developer using the tool today; future agent consumers (the `context` command, the planned MCP server) that need a stable contract for "which docs to inline." The output contract from CLAUDE.md is non-negotiable — stdout is data, stderr is diagnostics — and init's interactive UI must not violate it because the same prompt helper will be reused by commands whose stdout *does* carry data (auto-prompt fallback when a user runs e.g. `repograph context` before ever running `init`).

## Goals / Non-Goals

**Goals:**

- Polished first-run onboarding modeled on Ultracite / `@clack/prompts`: detection where possible, multi-select for agents, optional composed onboarding (first repo + workspace), styled summary screen.
- Establish the `[agents]` config schema as the durable contract for agent-doc discovery; presence of section = "configured."
- Provide a non-interactive variant suitable for CI, dotfile bootstrapping, and the MCP server's eventual setup tooling.
- Provide a shared auto-prompt helper so future commands (`context` first) can transparently route a user through agent selection on first use without duplicating prompt logic.
- Treat detection as a *suggestion*: detection preselects, user always has final say.
- Honor the existing output contract: stdout reserved for data; all interactive UI on stderr.

**Non-Goals:**

- The `context` command itself (Phase 4b — separate change).
- A per-repo `context_files` override (deferred indefinitely; init + agent IDs is the discovery contract).
- Reading or writing CLAUDE.md / AGENTS.md / etc. content from user repos (read-only is enforced when `context` lands; init never touches user docs).
- Migration of existing configs (the schema is additive — pre-init configs are valid and trigger first-run on next agent-consuming command).
- Discovery of agents beyond the v1 hardcoded set. New agents require a code change; the registry is intentionally not user-extensible.

## Decisions

### Decision: Use the `cliclack` crate for interactive UI

`cliclack` is a Rust port of `@clack/prompts` (which Ultracite uses) and provides intro / outro / multiselect / select / text / confirm / note / log / spinner primitives with the same box-drawn aesthetic. Alternatives:

- `dialoguer`: solid baseline but visually flat — no framed flow, no styled summary. Loses the polished feel we want for the first-run impression.
- `inquire`: rich, but the look is different from cliclack and the API surface is heavier than we need.
- Roll our own with `crossterm`: out of scope; reinvents what cliclack already does.

cliclack lands in `crates/repograph/Cargo.toml` only — the binary owns presentation, core stays terminal-free.

### Decision: "Init has been run" = presence of `[agents]` section, not a boolean flag

The natural TOML model is `[agents] selected = [...]`. If the section exists at all, init is considered done, regardless of whether `selected` is empty. This avoids:

- A separate `[meta] initialized = true` flag that drifts from reality.
- The "did the user actually finish init, or did they Ctrl-C halfway?" question — partial init writes nothing; full init writes the section once.

Empty `selected = []` is explicitly valid: a user who runs `init`, deselects everything, and confirms has opted out of agent docs. Future `context` invocations see `selected = []` and inline nothing without re-prompting.

### Decision: Agent registry is hardcoded in `repograph-core::agents`, not user-extensible

Agent IDs are an enum: `ClaudeCode`, `AgentsMd`, `Cursor`, `Aider`, `Windsurf`, `Copilot`. Each ID has one or more compiled-in file patterns. Rationale:

- The contract is between repograph and the agent toolchain ecosystem, not between repograph and each user's preferences. If Cursor changes its rules format, that's a `repograph` version bump, not a config edit.
- A user-extensible registry would re-introduce the per-repo configuration mess that we explicitly ruled out during exploration.
- Forward compatibility: adding a new agent is a one-line enum extension plus its pattern. Removing one needs a migration path (covered under Risks).

Trade-off: out-of-the-box support for a niche agent requires upstream PRs. Acceptable for v1; revisit if real demand emerges.

### Decision: Init composes existing primitives, never reimplements them

The optional "register a repo now" step calls `Config::add_repo` exactly as `repograph add` does. The optional "create a workspace / add to existing" step calls `Config::create_workspace` and `Config::add_members`. Errors from these primitives propagate as-is and are rendered with cliclack's error helper. Rationale:

- Outside-in TDD discipline: the same primitives are already tested at the core level. Duplicating their logic in the init flow would be a maintenance liability and risk drift.
- Composition keeps the dev plan honest: init's value-add is the *flow*, not new domain logic.

### Decision: cliclack writes to stderr — no theme override needed (resolved deviation)

`cliclack` 0.5.4's `term_write` helper unconditionally targets `Term::stderr()` for every `intro`/`outro`/`note`/`log::*`/prompt output (see `cliclack-0.5.4/src/lib.rs:328`). The "stdout is data, stderr is diagnostics" contract holds for free — no `Theme` writer override is needed.

This was originally specified as an override-and-test task group. Resolved during implementation by inspecting the cliclack source; the task group simplified to a documentation-only step. The output contract is still tested implicitly via the `init_no_prompt_emits_nothing_to_stdout` acceptance test, which confirms stdout stays empty during init (the non-interactive path) and indirectly validates that cliclack would not contaminate stdout in the interactive path.

If a future cliclack version changes this behavior, the regression surfaces as a failing acceptance test that asserts stdout is empty after an init flow.

### Decision: Non-interactive trigger semantics

Two situations:

1. **`repograph init` with `--no-prompt`** (TTY or not): requires `--agents <list>`. Validates the list against the agent registry, writes `[agents] selected = [...]`, runs no prompts. Useful for dotfile bootstrapping and CI.
2. **`repograph init` with no flags, stdout not a TTY**: errors with exit code `2` and message instructing the user to either pass `--no-prompt --agents <list>` or run in an interactive shell. We do not silently fall back to "all agents" or "default agent."

The auto-prompt fallback (when another command discovers `[agents]` missing) follows the same rule: TTY → prompt; non-TTY → exit `2` with `"agents not configured; run \`repograph init\`"`.

### Decision: Detection scope is `$HOME`-level, not project-level

In `init`'s first-run mode we preselect agents based on the presence of well-known directories or files in the user's home:

| Agent ID      | Detection signal                                                  |
|---------------|-------------------------------------------------------------------|
| `claude-code` | `~/.claude/` exists, or `~/.config/claude/` exists                |
| `cursor`      | `~/.cursor/` exists                                               |
| `aider`       | `~/.aider/` exists, or `~/.aider.conf.yml` in `$HOME`             |
| `windsurf`    | `~/.codeium/windsurf/` exists                                     |
| `copilot`     | `~/.config/github-copilot/` exists, or `gh` extension installed   |
| `agents-md`   | (no $HOME signal — never preselected; user must opt in)           |

We deliberately do NOT scan the current directory for `CLAUDE.md` / `AGENTS.md` — init is a setup command, not a repo audit, and the cwd at init time may be unrelated to the repos the user will register later. Detection is best-effort: false positives (directory exists but unused) are harmless because the user can deselect.

### Decision: Auto-prompt helper signature and placement

```rust
// crates/repograph/src/prompt.rs
pub fn ensure_agents_configured(
    config: &mut Config,
    config_dir: &Path,
) -> Result<(), RepographError>;
```

- Called by any command (init itself, plus future `context`) that needs `[agents]` populated.
- If section already present → no-op, returns `Ok`.
- If section missing AND stdout is a TTY → runs the same agent multiselect sub-flow used by `init`, mutates `config`, calls `config.save(config_dir)`, returns `Ok`.
- If section missing AND stdout is NOT a TTY → returns a typed `RepographError::NeedsInit { .. }` variant (new) mapped to exit code `2`, with a message naming `repograph init`.

Placement in the binary crate is deliberate — the helper depends on cliclack and on `OutputMode`-style TTY detection, both of which are presentation concerns. Core stays clean.

### Decision: Settings-panel mode for re-init

When `[agents]` already exists, `repograph init` shows a top-level select:

```
What would you like to do?
  ○ Update agent selection
  ○ Register another repo
  ○ Manage workspaces
  ○ Reset everything
  ○ Cancel
```

This makes `init` discoverable as the entry point for users who don't memorize the full subcommand list. "Manage workspaces" hands off to a sub-flow that wraps `Config::create_workspace` / `Config::add_members` / `Config::remove_members` / `Config::remove_workspace`. "Reset everything" prompts for explicit confirmation, then writes a fresh empty `Config` to disk (the equivalent of `rm config.toml` followed by `repograph init` first-run).

### Decision: `tracing` discipline matches the rest of the codebase

Init follows the established pattern from `commands/status.rs` and the `logging.md` rule:

- `#[tracing::instrument(skip(args), fields(no_prompt = args.no_prompt, ...))]` on `run()`.
- `debug!` at entry, `info!` at completion with `agents = ?selected`, `error!` on propagated failures.
- Cliclack output is UI and does not go through `tracing`; tracing logs are independent diagnostics, useful when piped through a log aggregator.

### Resolved deviation: `--no-prompt` requires `--agents` is enforced by clap, not by a runtime check

The original plan mentioned a runtime usage error for `--no-prompt` without `--agents`. In practice, expressing the dependency via clap's `#[arg(long, requires = "agents")]` is cleaner: clap catches the misuse at argument parsing time, exits with the documented code `2`, and produces a standard usage message that names both flags. The `run_non_interactive` path is correspondingly simpler — it relies on the clap-level invariant that `args.agents` is `Some` when `--no-prompt` is set, and treats an empty `--agents ""` value as the explicitly-empty selection (`selected = []`).

### Resolved deviation: `dirs` is added to the binary crate

`dirs` was already on `repograph-core` (used by `Config::default_dir`). The detection helper in `prompt.rs` needs `dirs::home_dir()` from the binary side. Adding `dirs = "5"` to `crates/repograph/Cargo.toml` keeps prompt concerns inside the binary crate (where they belong) without re-exporting filesystem helpers through core. This was not explicitly listed in the proposal; the addition is mechanical and aligned with the existing module layout decision.

### Decision: project root is a one-time setup question persisted in `[settings]`

A user's "where do I keep my projects" answer changes rarely (often never after first install). Probing the filesystem fresh on every repo-registration is both noisy (empty candidate dirs leak through, e.g. `~/Projects (0 repos)`) and against the industry norm — `ghq` reads `git config ghq.root`, JetBrains stores the IDE projects folder in user preferences, GitHub Desktop has a "Default repository location" preference. Repograph follows the same pattern:

1. **`[settings] projects_root`** in the config — a single `Option<PathBuf>`. Presence means "the user has answered this question"; absence means "ask next time."
2. **Env var override**: `REPOGRAPH_PROJECT_ROOT` wins over the config value, mirroring `REPOGRAPH_CONFIG_DIR` ergonomics. Useful for CI / sandbox testing / dotfile parity.
3. **First-run flow asks once**, after agents and before the optional repo step. Detected roots that contain at least one git repo are surfaced as primary options; "Enter a custom path..." opens a free-form autocompleted input; "Skip" persists `projects_root = None` so the answer is recorded but no path is stored.
4. **Subsequent runs never re-ask**: the `pick_projects_root_step` early-returns when `effective_projects_root(config)` is `Some(_)`. Repo registration scans the stored root directly.
5. **Settings-panel "Change project root"** action lets the user revisit the choice from `repograph init` on an already-configured install. When the env var is active, the panel logs a warning that the env will override the stored value.

The probing now serves only as a *seed* for the first-run prompt — detected roots become preselect options. Empty candidates (the `~/Projects (0 repos)` case) are filtered out by `discover_project_roots` so they never reach the picker.

Alternatives considered:

- **Probe-every-time (no persistence).** What v0 of this change did. Rejected — produces noise, has no answer to "user keeps projects in a non-standard location," and violates the "ask once, remember" UX every mature tool uses.
- **Store a list of roots.** Power users sometimes split personal vs work into separate parent folders. Rejected for v1 — ghq stores a single root and developers cope; the free-form input + autocomplete handles the "occasional cross-folder" case. Can be revisited if real demand emerges.
- **Walk-up-from-CWD pattern (direnv-style).** Doesn't apply — repograph asks where to look BEFORE the user is in a specific repo. The CWD at init time is unrelated to where repos live.

### Decision: bulk multi-repo registration via multiselect over the projects root

A developer with a populated projects root (the common case — 5–30 repos under `~/IdeaProjects` or `~/code`) needs to onboard most of them at once. The original v1 design had the repo-registration step register exactly one repo per init invocation; users with N repos had to re-run init N times. That contradicts the "industry pattern" decision we already locked in for projects-root (ghq lets you `ghq get` bulk; JetBrains scans and offers all detected). Fold the multi-add into the same change so the UX shipped end-to-end matches the persisted-root that drives it.

The new repo-registration step is two phases:

1. **Multiselect over scanned candidates** — `cliclack::multiselect(...).required(false)` lists every direct child of the projects root that has a `.git` entry and isn't already registered, with no preselection. The user picks 0..N. Submitting with 0 is a valid "no thanks." Picked paths register with their directory basename as the default name.
2. **Free-form add-another loop** — after the multiselect (or unconditionally if no candidates exist), a `confirm("Register a repo at a custom path?")` gates a free-form input with autocomplete; on `yes`, the user types a path, the repo registers, and the confirm fires again until they decline.

Bulk-write of N>0 repos hits two failure modes that the single-add path didn't have to think about:

- **Basename collision** (two scanned dirs share a basename, or the basename matches a pre-existing registry entry). Solved by inline `log::error` + a single one-shot "Different name?" prompt; persistent conflicts skip the path with a stderr log line so the rest of the batch proceeds.
- **Path went away between scan and write** (race, symlink rot). The scan already filtered by `.git` presence and `add_repo`'s path-uniqueness check catches duplicates, so this is rare. Treated the same as collision — log + skip.

The settings-panel "Register another repo" action shares the same `register_repos_step` function — the menu item itself signals intent so no outer "Register repos now?" confirm fires there. First-run does ask the outer confirm so the user can decline the whole step.

Bulk workspace assignment is the natural follow-up — and importantly, "a workspace" is too narrow for the way developers actually slice their work. A repo often belongs to both a domain workspace (`backend`) and a team workspace (`team-alpha`); forcing the user to re-enter the init flow once per workspace target would make the bulk improvement meaningless.

The shipped shape (post-deviation, see "Resolved deviation" below): a single outer `confirm("Add these N repos to workspaces?")` (plural) gates the step. On `yes`, the user passes through two phases:

1. **Workspace prep — optional create-new loop** — gated by a `Create new workspaces first?` confirm when existing workspaces were available, and entered directly when none exist (so first-run users aren't stuck without targets). Each iteration prompts for a name (validated + duplicate-checked via the existing `prompt_workspace_name` helper), calls `Config::create_workspace`, then asks `Create another workspace?` (default `no`) to break or continue. This phase seeds the target pool.
2. **Per-repo assignment** — for each repo in registration order, render a `multiselect("Workspaces for '<repo>'")` over the full target pool (existing + just-created), no preselection, `.required(false)`. The user picks the workspaces that specific repo should join. Empty submission leaves that repo unassigned and proceeds to the next repo.

Each picked (repo, workspace) pair triggers `Config::add_members(workspace, &[repo])`. The whole step persists with a **single** `Config::save` at the end so partial failure mid-loop doesn't leave half-written membership on disk. Empty target pool (no existing AND none created) yields a `WARN` log + clean save + return. Per-repo empty pick is silently a no-op for that repo. Success log uses the singular form `"added '<repo>' to '<ws>'"` when exactly one repo lands in exactly one workspace, otherwise a multi-line `workspace assignments:` block listing each assigned repo with its chosen workspaces. When zero repos receive picks, an `INFO` log `"no workspace assignments made"` replaces the success block.

Sub-alternatives considered for the workspace step:

- **Single-target `select` + outer "Add to another workspace?" loop.** Simpler code but N+1 prompts per workspace; punishes the user with many workspaces. Per-repo multiselect is 1 prompt per repo regardless of how many workspaces they end up in.
- **Multiselect that includes a synthetic "Create new..." pseudo-item.** cliclack `multiselect` items are typed values; branching on a sentinel inside the picker is awkward and mixes navigation with action. Split-phase (create-loop-first then per-repo-multiselect) is cleaner.
- **Preselect all existing workspaces in the multiselect.** Same reason we rejected preselect-all on the repo multiselect: opt-in beats opt-out for write operations.

Alternatives considered:

- **Defer bulk to a follow-up change.** Tempting but wasteful — the change is still pre-archive, the helpers are small, and shipping a "now ask for projects-root, then register repos one by one" UX would be a strictly worse intermediate state for a user who walks the wizard today.
- **`multiselect.initial_values(all_candidates)`** (preselect everything). Rejected — too aggressive; a user who runs init in a folder with 30 dirs would risk accidentally registering all of them. Opt-in beats opt-out for write operations.

#### Resolved deviation: Per-repo iteration instead of matrix-style multi-target assignment

The originally shipped shape collected workspace targets *once* via a multiselect over existing workspaces (optionally extended by a create-new loop), then assigned **every registered repo to every picked target** (a matrix). A user surfaced the friction immediately: "i want to choose which repo to put into which workspace." The matrix shape coupled every repo's membership to the same workspace decision, which is fine when N=1 or all repos belong to the same workspaces but is exactly wrong when registering a batch of repos that fan out into different workspaces (the common case for a polyrepo workspace).

The fix inverts the iteration:

1. **Create-new loop moved first** — the user creates whatever new workspaces they need *before* assignment, so phase 2 has the full target pool available. This loses no expressiveness from the prior shape (which created workspaces after the existing-multiselect) and adds the flexibility of routing different repos into different new workspaces.
2. **Per-repo multiselect replaces the single multi-target multiselect** — for each repo in registration order, the user picks that repo's workspaces (zero, one, or many) from the full pool. Empty picks are a per-repo no-op; the loop continues to the next repo.
3. **Success log reshape** — the prior `"added N repos to M workspaces: name1, name2"` summary doesn't describe per-repo assignments. The new log uses the singular `"added '<repo>' to '<ws>'"` form when exactly one (repo, ws) pair lands, and a multi-line `workspace assignments:` block listing each assigned repo with its chosen workspaces otherwise.
4. **Same-workspace-for-every-repo is still expressible** — the user picks the same workspace in each per-repo picker. The cost is N quick multiselects instead of one, but the per-repo picker is small (one screen, full workspace list), so the keystroke cost is minimal. The expressivity gain (per-repo routing) dominates.

The prior "alternative considered — per-repo workspace prompt inside the multiselect loop, rejected because 10 repos with the same workspace would mean 10 identical prompts" is the reason the matrix shape was originally chosen. The deviation note acknowledges that the prior rejection underweighted the per-repo-routing case, which is the actual common shape for a polyrepo init.

### Decision: Manage-workspaces `Create` chains into bulk-add; `Add members` uses multiselect

A user surfaced the friction mid-implementation: "now I can create more workspaces… but I lost the ability to add repos to them." The settings panel's `Manage workspaces` sub-flow was correct in primitives (Create → Add members → Remove members → Delete) but wrong in shape — `Create` ended at the name prompt, leaving the user to navigate back through the menu and use `Add members` once per repo via a single-select. The mental model is "I am creating a workspace AND populating it"; making that a two-trip chore is the same anti-pattern as the original "one repo per init" we already fixed for repo registration.

Two fixes, one helper:

1. **Extract `add_repos_to_workspace(config, config_dir, ws_name)`** — multiselect over the registry filtered to repos not already in the workspace (no preselection); empty submission is a valid no-op; success log uses singular wording when exactly one repo lands.
2. **`WsAction::Create`** — after `create_workspace` + `save`, render `confirm("Add repos to '<name>' now?")` with `initial_value(true)` (the menu choice already signalled intent; default `yes` matches user expectation) and chain into `add_repos_to_workspace` on `yes`. Skip the confirm entirely when the registry is empty (nothing to add).
3. **`WsAction::AddMembers`** — replace the prior `pick_repo` single-select with `add_repos_to_workspace`, so N repos land in one pass. Already-member repos are filtered from the picker so the user never sees them as choices.

When all registered repos are already members (or the registry is empty for AddMembers), emit a `WARN` cliclack log explaining the no-op reason rather than rendering an empty picker. This matches the rest of the init UX where the "wait, nothing to do?" state is named explicitly.

Alternatives considered:

- **Keep AddMembers single-select; only chain Create→add.** Half the win — a user with an existing workspace still pays the per-repo navigation tax. Replacing single-select with multiselect costs ~10 lines and removes the asymmetry.
- **Force a non-empty selection.** Rejected — `multiselect.required(false)` lets the user back out cleanly without an extra "Cancel" path. The empty no-op is fine; the alternative is forcing a Ctrl-C escape, which is hostile.
- **Skip the confirm in Create and always render the multiselect.** Rejected — when the user creates a workspace as a placeholder ("I'll fill it in later"), the immediate multiselect is intrusive. The default-yes confirm preserves both flows with one ↵ for the common case.

### Decision: repo-path input gets filesystem autocomplete and project-root discovery

The first-run "register a repo" step started as a single free-form `cliclack::input("Path to repository")`. Two enhancements were added mid-implementation to honor the "make easy things easy" half of the production-grade rule:

1. **Filesystem autocomplete on the free-form input** — wired via `cliclack::Input::autocomplete(prompt::path_suggestions)`. `path_suggestions(&str) -> Vec<String>` is a `cliclack::Suggest` source (the `Fn` blanket impl) that expands `~`, splits the input into parent + prefix, scans the parent directory, filters to directories, honors the hidden-file convention (`.foo` surfaces only when the prefix starts with `.`), and returns each match as an absolute path with a trailing `/` so consecutive Tab presses drill deeper.
2. **Project-root discovery + repo picker** — `prompt::discover_project_roots(home)` probes a fixed list of common conventions (`IdeaProjects`, `Projects`, `projects`, `dev`, `code`, `Code`, `work`, `src`, `repos`) under `$HOME` and returns those that exist. `prompt::scan_git_repos(root)` walks the immediate children of a root and surfaces the ones that contain a `.git` entry (directory or worktree-marker file). The init flow uses these to render a select before falling back to the free-form input: if exactly one root is detected, scan it directly; if multiple roots exist, ask which one first; in either case the resulting picker has an "Other path..." escape that drops to the autocomplete-enabled input. Already-registered paths are filtered out of the picker so conflicts don't surface there.

These are pure UX wins backed by isolated unit tests on the helpers (`tempdir`-driven). The free-form input still works for users with non-standard layouts. The agent registry, schema, and exit-code contracts are unaffected.

Alternatives considered:

- **Defer to a follow-up change.** Possible, but the change is still pre-archive and the helpers are small and well-tested. Bundling avoids a second round-trip through propose/apply/archive for a UX improvement on the same flow.
- **Tab-completion via the user's shell instead of cliclack.** Doesn't apply — clap-generated shell completions help with CLI args, not with values typed into a cliclack prompt.
- **Recursive scan of project roots.** Rejected — a one-level scan covers the common case (devs lay repos out flat under `~/IdeaProjects`), is fast, and avoids surprising the user with deeply-nested clutter. A user with nested layouts uses "Other path..." with autocomplete.

## Risks / Trade-offs

**Risk**: Interactive flow is hard to e2e test — cliclack reads from `stdin` and writes formatted output to a TTY; capturing both reliably from `assert_cmd` is brittle.
**Mitigation**: Acceptance tests cover only the non-interactive (`--no-prompt --agents …`) path via `assert_cmd`. The interactive path is covered by a manual validation script documented in this file (see "Manual Validation Script" below) that must be walked before archive. Unit tests cover the agent registry, config serde, and detection helpers in isolation.

**Risk**: cliclack behavior on non-standard terminals (Windows legacy console, CI runners, screen readers, `TERM=dumb`).
**Mitigation**: TTY detection via `is-terminal` (already a dep) gates entry into interactive mode. Non-TTY paths require `--no-prompt`. We do not attempt graceful degradation within cliclack — if it can't draw frames, the user shouldn't be in interactive mode.

**Risk**: Adding a new agent ID later is a breaking config schema change for users on a build that doesn't know the new ID (downgrades).
**Mitigation**: Serde rejects unknown enum variants by default, which produces `RepographError::ConfigParse` — clean failure, not silent corruption. We accept that downgrading after upgrade requires editing config or re-running init. Document in the README's upgrade notes.

**Risk**: Removing an agent ID in a future version breaks configs that reference it.
**Mitigation**: We commit to never removing an agent ID without a deprecation period (one minor release where the ID is accepted with a `warn!` log and routed to a no-op). Documented as a stability promise in the agent-registry module doc comment.

**Risk**: The detection step gives a false sense of "we figured it out" — user accepts the preselection without thinking.
**Mitigation**: The multiselect screen always requires explicit ↵ confirmation; we never auto-confirm. The summary screen at the end echoes the final selection, so a user who blindly accepts still sees what they agreed to.

**Risk**: cliclack's stderr theme override may not be a stable API across cliclack versions.
**Mitigation**: Pin `cliclack = "=<version>"` (exact) in the binary crate's `Cargo.toml`. Bumping cliclack requires verifying the writer override still works in CI manual smoke. Documented in the production-grade rule's "dependency hygiene" intent.

**Risk**: Composing `add` and `workspace` inside init duplicates the validation paths (path canonicalization, name uniqueness, workspace name policy). If those primitives' errors change shape, init's rendering must follow.
**Mitigation**: `Config::add_repo`, `Config::create_workspace`, and `Config::add_members` already return typed `RepographError` variants. Init renders those errors via cliclack `log::error()` and re-prompts the relevant step. Tests in the non-interactive path exercise each error variant.

## Manual Validation Script

Walked once before archive, captured as a checklist comment in the archive PR:

1. **Fresh install, no config**: `cargo run -- init` in an empty `XDG_CONFIG_HOME`. Verify detection preselects based on actual host state. Walk through agents → skip repo → skip workspace → confirm summary shows agents.
2. **Fresh install, with repo + workspace**: `cargo run -- init`, register the cwd repo, create a new workspace named `acme`, add the repo to it. Verify config.toml has `[repo.<name>]`, `[workspace.acme]` with member, and `[agents]` populated.
2a. **Bulk multiselect from projects root + per-repo workspace routing**: with a projects root containing ≥3 unregistered repos and ≥2 existing workspaces (e.g. `backend`, `frontend`), run `cargo run -- init`, select all 3 repos in the multiselect, confirm the workspace prompt, decline `Create new workspaces first?`, then walk the three per-repo pickers: tick `backend` for the first repo, `frontend` for the second, and both `backend` and `frontend` for the third. Verify all 3 `[repo.*]` entries appear AND each workspace's `members` array reflects the per-repo picks (NOT a matrix). Re-run init — verify the repo multiselect now shows zero unregistered candidates (already-registered are filtered). The success log should be the multi-line `workspace assignments:` block.
2b. **Per-repo routing with create-new alongside existing pick**: with ≥1 existing workspace (`acme`), register 2 repos in one pass, confirm the workspace prompt, confirm `Create new workspaces first?`, create a fresh workspace `team`, decline "Create another?", then walk the two per-repo pickers: tick `acme` only for the first repo, and tick both `acme` and `team` for the second. Verify `acme.members` contains both names but `team.members` contains only the second name; the config saved once at the end.
3. **Re-init settings panel**: with config present, `cargo run -- init`. Verify the settings panel renders, "Update agent selection" pre-checks the current selection, deselect/reselect produces the right diff in config.
4. **Non-interactive happy path**: `cargo run -- init --no-prompt --agents claude-code,cursor` against an empty config. Verify exit `0`, config has exactly `[agents] selected = ["claude-code", "cursor"]`.
5. **Non-interactive bad agent**: `cargo run -- init --no-prompt --agents bogus` → exit `2`, stderr names the unknown ID.
6. **Non-TTY without flags**: `echo | cargo run -- init` → exit `2`, stderr says to use `--no-prompt --agents` or run in a TTY.
7. **Reset everything**: re-init → Reset → confirm → verify config.toml is the byte equivalent of an empty config (`{}` round-trip).
8. **Auto-prompt fallback smoke**: temporarily wire a stub command that calls `prompt::ensure_agents_configured` and verify it routes to the agent sub-flow when config has no `[agents]` and stdout is a TTY.
9. **Manage workspaces → Create populates new workspace in one step**: with ≥2 registered repos, run `cargo run -- init` → `Manage workspaces` → `Create` → enter name `acme`. Accept the default-yes "Add repos to 'acme' now?" confirm. Verify the multiselect lists all registered repos with none preselected; tick two; submit. Verify the config gained `[workspace.acme]` with `members = ["repo-a", "repo-b"]` and the success log read "added 2 repos to 'acme'".
10. **Manage workspaces → Add members shows only non-members**: with workspace `acme` containing `repo-a` and the registry also containing `repo-b`, `repo-c`, run `Manage workspaces` → `Add members` → pick `acme`. Verify the multiselect renders `repo-b` and `repo-c` only (no `repo-a`); tick both and submit. Verify `members = ["repo-a", "repo-b", "repo-c"]` (order preserved) and the success log reads "added 2 repos to 'acme'".
11. **Manage workspaces → Add members no-op warns when all members already in**: with a workspace where every registered repo is a member, run `Add members` → pick that workspace. Verify no multiselect renders; a WARN log line names "all registered repos are already members — '<ws>' unchanged"; control returns to the settings panel with no `Config::save`.
