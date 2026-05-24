# init-command Specification

## Purpose

The `repograph init` capability owns the interactive onboarding flow that declares the user's agent toolchain, the `[agents]` and `[settings]` config schema, the built-in agent ID → file pattern registry, and the shared auto-prompt fallback consumed by future agent-aware commands. It composes (rather than replaces) the registry-core and workspace-support primitives so first-run setup and ongoing settings management converge through a single command.

## Requirements

### Requirement: Agent registry maps well-known IDs to file patterns

The system SHALL define a built-in registry of agent identifiers, each associated with one or more glob-style file patterns describing where that agent stores its rules within a repository. The registry SHALL be hardcoded in the core crate (`repograph-core::agents`) and SHALL NOT be user-extensible. Agent identifiers SHALL be serialized as kebab-case strings in TOML and JSON.

The v1 registry SHALL contain exactly the following entries:

| Agent ID      | File patterns                                                       |
|---------------|---------------------------------------------------------------------|
| `claude-code` | `CLAUDE.md`                                                         |
| `agents-md`   | `AGENTS.md`                                                         |
| `cursor`      | `.cursor/rules/*.md`, `.cursorrules`                                |
| `aider`       | `CONVENTIONS.md`                                                    |
| `windsurf`    | `.windsurfrules`                                                    |
| `copilot`     | `.github/copilot-instructions.md`                                   |

#### Scenario: Each well-known agent ID resolves to its patterns

- **WHEN** the agent registry is queried for each of the v1 IDs in turn
- **THEN** the returned pattern list is non-empty and matches the table above; querying an unknown ID returns `None`

#### Scenario: Agent IDs round-trip through TOML serialization

- **WHEN** a `Config` with `[agents] selected = ["claude-code", "cursor"]` is written to disk and reloaded
- **THEN** the reloaded `Config` has the same two agent IDs in the same order, deserialized as the typed enum (not as raw strings)

#### Scenario: Unknown agent ID in config produces a clean parse error

- **WHEN** a config file contains `[agents] selected = ["claude-code", "bogus"]` and any subcommand runs
- **THEN** the exit code is `1` and stderr names the offending value `bogus` as not a recognized agent

### Requirement: Config persists the user's agent selection

The `Config` type SHALL gain an `agents` field representing an `[agents]` TOML section with a `selected: Vec<AgentId>` member. The field SHALL be optional in the serde layer (a config without `[agents]` deserializes successfully with `agents = None`) and SHALL be omitted from serialization when `None`. When present, `selected` SHALL be serialized as a TOML array of kebab-case strings in the order the user selected them.

#### Scenario: Config without [agents] section loads as not-configured

- **WHEN** a config file at `<dir>/config.toml` contains only `[repo.*]` and `[workspace.*]` sections
- **THEN** `Config::load(dir)` succeeds, `config.agents()` returns `None`, and all other fields load unchanged

#### Scenario: Config with empty [agents] selected is treated as configured-but-empty

- **WHEN** a config file contains `[agents]\nselected = []` and `Config::load(dir)` is called
- **THEN** `config.agents()` returns `Some(Agents { selected: vec![] })` — distinct from `None`

#### Scenario: Saving Config with no agents omits the section

- **WHEN** a `Config` with `agents = None` is saved
- **THEN** the resulting TOML file contains no `[agents]` section header

#### Scenario: Saving Config with agents preserves selection order

- **WHEN** a `Config` with `selected = [Cursor, ClaudeCode]` (in that order) is saved
- **THEN** the TOML file contains `selected = ["cursor", "claude-code"]` in that exact order; a round-trip load produces the same ordered vector

### Requirement: Init detects configuration state and routes to first-run or settings-panel flow

The CLI SHALL accept a `repograph init` subcommand. When invoked, the system SHALL inspect the loaded config: if the `[agents]` section is absent, the command SHALL run the first-run flow; if present, the command SHALL run the settings-panel flow. The two flows SHALL be visually distinct and SHALL be entered automatically based on config state with no flag required from the user.

#### Scenario: First invocation on empty config enters first-run flow

- **WHEN** no config file exists or the existing config has no `[agents]` section, the user runs `repograph init`, and stdout is a TTY
- **THEN** the command renders the first-run intro screen and proceeds through agent selection; on completion `[agents]` is present in the saved config

#### Scenario: Subsequent invocation enters settings-panel flow

- **WHEN** the config has an `[agents]` section (selection may be empty or populated), the user runs `repograph init`, and stdout is a TTY
- **THEN** the command renders the settings-panel select with options `Update agent selection`, `Register another repo`, `Manage workspaces`, `Reset everything`, `Cancel`

### Requirement: First-run flow walks agent selection then optional repo and workspace composition

When in first-run mode and stdout is a TTY, the system SHALL guide the user through three ordered steps, in this order: (1) agent multiselect with detection-based preselection, (2) optional first repo registration, (3) optional workspace assignment for that repo. Each step SHALL be skippable. The final screen SHALL summarize the resulting state (selected agents, registered repos count, workspaces count) before exiting.

The repo registration step, when selected, SHALL invoke `Config::add_repo` with the same validation as the existing `repograph add` command — path must be a real git repository, name must not conflict with an existing entry. The workspace step, when selected, SHALL invoke `Config::create_workspace` and/or `Config::add_members` with the same validation as the existing `repograph workspace` commands.

#### Scenario: User completes agents-only first run

- **WHEN** a user runs `repograph init` against an empty config, selects two agents, skips the repo step, and confirms the summary
- **THEN** the saved config contains exactly `[agents] selected = [...]` with those two agents in selection order; no `[repo.*]` or `[workspace.*]` sections; exit code is `0`

#### Scenario: User completes full first run with repo and new workspace

- **WHEN** a user runs `repograph init`, selects one agent, registers a real git repo at a tempdir path as `my-repo`, creates a new workspace `team`, and assigns the repo
- **THEN** the saved config contains `[agents] selected = [...]`, one `[repo.my-repo]` entry with the canonical path, one `[workspace.team]` entry with `members = ["my-repo"]`; exit code is `0`

#### Scenario: User completes first run skipping both optional steps

- **WHEN** a user runs `repograph init`, selects agents, then chooses "Skip" on both the repo and workspace steps
- **THEN** the saved config contains only `[agents] selected = [...]`; exit code is `0`

#### Scenario: Repo registration error during init reprompts in place

- **WHEN** during the first-run repo step, the user enters a path that is not a git repository
- **THEN** the command renders an inline error matching the `repograph add` error message, and re-prompts for the path without exiting the init flow

#### Scenario: Workspace name conflict during init reprompts in place

- **WHEN** during the first-run workspace step, the user attempts to create a workspace named identically to an existing one
- **THEN** the command renders an inline `RepographError::Conflict` message and re-prompts for the workspace name without exiting the init flow

### Requirement: Project root is persisted in `[settings]` and overridable via env var

The system SHALL persist the user's declared "where I keep my projects" answer in a new `[settings]` section of the config TOML, as `projects_root: Option<PathBuf>`. The presence of the `[settings]` section (regardless of whether `projects_root` is set) SHALL mean "the user has answered the projects-root question"; absence SHALL mean "ask the user next time it's needed."

Effective value SHALL be resolved with the precedence: `REPOGRAPH_PROJECT_ROOT` env var (when set and non-empty) → `config.settings().projects_root` → `None`. The first-run init flow SHALL ask this question once, after agent selection and before optional repo registration, with detected roots (filtered to those containing at least one git repo) surfaced as primary options alongside "Enter a custom path..." and "Skip" choices. Subsequent init invocations SHALL NOT re-prompt; the settings panel SHALL expose a "Change project root" action for explicit reconfiguration.

#### Scenario: First-run flow asks projects-root after agents and persists the answer

- **WHEN** a user with no existing config runs `repograph init` in a TTY, selects agents, then picks one of the detected project roots
- **THEN** the saved config contains a `[settings]` section with `projects_root = "<chosen path>"`, in addition to `[agents]`

#### Scenario: First-run flow with "Skip" persists settings section with no projects_root

- **WHEN** a user runs `repograph init` and selects "Skip — I'll set this later" at the projects-root step
- **THEN** the saved config contains a `[settings]` section header (signalling "answered") with no `projects_root` field; subsequent init runs do not re-ask the question

#### Scenario: Subsequent init invocations do not re-ask when projects_root is set

- **WHEN** a user has previously completed init with `projects_root = "/home/user/code"` and runs `repograph init` again
- **THEN** the settings panel renders (not the first-run flow); no "Where do you keep your projects?" prompt fires unless the user explicitly chooses "Change project root"

#### Scenario: Env var overrides stored config value

- **WHEN** the config has `[settings] projects_root = "/from/config"`, the environment has `REPOGRAPH_PROJECT_ROOT=/from/env`, and any agent-consuming command queries the effective root
- **THEN** the effective value is `/from/env`; the config value is left unmodified

#### Scenario: Empty env var value falls through to config

- **WHEN** `REPOGRAPH_PROJECT_ROOT` is set to the empty string and the config has `[settings] projects_root = "/from/config"`
- **THEN** the effective value is `/from/config` (an empty env var does not shadow the config)

#### Scenario: Settings panel "Change project root" prompts and persists

- **WHEN** a user with a configured projects-root runs `repograph init` → "Change project root", picks a different value, and confirms
- **THEN** the saved config's `[settings] projects_root` is updated to the new value; all other sections (`[agents]`, `[repo.*]`, `[workspace.*]`) are unchanged

#### Scenario: Settings panel warns when env var would override the stored value

- **WHEN** `REPOGRAPH_PROJECT_ROOT` is set in the environment and the user runs `repograph init` → "Change project root"
- **THEN** a `WARN`-level cliclack log line is emitted naming `REPOGRAPH_PROJECT_ROOT` and explaining it overrides whatever the user picks; the prompt still renders and persists the user's choice

### Requirement: Repo registration step supports bulk multiselect plus filesystem autocomplete

When the first-run repo step (or the settings-panel "Register another repo" action) prompts the user for repositories, the system SHALL offer the following UX layered on top of the free-form input:

1. **Bulk multiselect from the persisted projects root** (env var → `[settings] projects_root` → none): scan the resolved root's direct children for entries that contain a `.git` directory or worktree-marker file, exclude any whose canonical path is already registered, and render a `multiselect` listing all unregistered candidates with no preselection. The multiselect SHALL allow zero, one, or many selections. Submitting with zero selected SHALL NOT register anything from this phase. Submitting with N>0 selected SHALL register each selected path as `[repo.<basename>]` using the directory basename as the default name. If no root is known, or the scan returns no unregistered entries, skip directly to step 2.
2. **Free-form add-another loop with filesystem autocomplete**: prompt the user (via a `confirm`) to register additional repos at custom paths outside the projects root. On `yes`, render `cliclack::Input` with autocomplete suggestions sourced from a filesystem-aware closure that expands `~`, scans the parent directory of the typed prefix, filters to directories, returns absolute paths with a trailing `/`, and honors the hidden-file convention (entries beginning with `.` surface only when the user typed a prefix beginning with `.`). Then re-confirm to loop until the user declines.

The repo-registration step SHALL NOT itself prompt for a project root or run discovery — the projects-root is owned by the dedicated step in the first-run flow and the settings panel.

All paths SHALL converge on a single canonical absolute path produced by `repograph_core::validate_git_repo` (for free-form input) or returned directly from the scan (for multiselect picks). Validation failures at any layer SHALL render an inline error and re-prompt without exiting the init flow.

Basename collisions during bulk registration SHALL render an inline error naming the conflict and prompt the user once for an alternative name. If the alternative also conflicts (or matches the original), that path SHALL be skipped with a stderr log message; the flow SHALL NOT abort.

#### Scenario: Persisted projects root drives the multiselect picker

- **WHEN** the config has `[settings] projects_root = "/home/user/code"`, that directory contains at least one unregistered git repo, and the user runs `repograph init` → "Register another repo"
- **THEN** the command scans `/home/user/code` and renders a `multiselect` labelled "Repositories in /home/user/code" listing the unregistered repos with no preselection; submitting with selections registers each as a `[repo.<basename>]` entry

#### Scenario: Bulk multiselect registers N selected repos in one pass

- **WHEN** the projects root contains 5 unregistered git repos and the user selects 3 of them in the multiselect
- **THEN** the saved config gains exactly 3 `[repo.<basename>]` entries with canonical paths; the unselected repos are NOT registered

#### Scenario: Empty multiselect submission is valid

- **WHEN** the user reaches the multiselect, selects zero items, and submits
- **THEN** no repos are registered from this phase; the flow proceeds to the "register a repo at a custom path?" loop

#### Scenario: Basename collision in bulk prompts for an alternative name once

- **WHEN** the user picks two repos in the multiselect whose basenames collide (or the basename collides with an existing registry entry)
- **THEN** the first registration uses the basename; the conflicting entry renders an inline error naming the conflict and prompts once for an alternative name; on a unique alternative the repo is registered, otherwise the path is skipped with a stderr log message and the flow continues

#### Scenario: No persisted projects root falls through to free-form-only loop

- **WHEN** the config has no `[settings] projects_root` (set to `None` or section absent), no env override is active, and the user confirms "Register repos now?"
- **THEN** the multiselect phase is skipped; the command prompts directly with the free-form path input (with filesystem autocomplete); the "register another?" confirm loop continues until the user declines

#### Scenario: Env-overridden projects root drives the multiselect

- **WHEN** the config has no `[settings] projects_root`, `REPOGRAPH_PROJECT_ROOT=/home/user/work` is set, and the user runs the repo-registration step
- **THEN** the command scans `/home/user/work` and renders the multiselect, treating the env value as the effective root

#### Scenario: Multiselect excludes already-registered repos

- **WHEN** the user's `~/IdeaProjects` contains repos `api`, `ui`, and `legacy`, the registry already contains `legacy` at that canonical path, and the user runs `repograph init` and chooses to register a repo
- **THEN** the multiselect rendered for `~/IdeaProjects` lists `api` and `ui` and does NOT list `legacy`

#### Scenario: Free-form add-another loop registers out-of-root repos

- **WHEN** after the multiselect the user confirms "Register a repo at a custom path?", enters a free-form path, completes registration, then declines the next "Register another?"
- **THEN** the additional repo is registered with its basename; the loop exits with both multiselect and free-form additions present in the saved config

#### Scenario: Filesystem autocomplete suggests directory entries

- **WHEN** the user is at the free-form path input and types a partial path such as `/home/user/I`
- **THEN** the autocomplete popup lists directory entries of `/home/user/` whose names begin with `I` (each rendered as an absolute path ending in `/`); files and entries failing the hidden-file convention are excluded

#### Scenario: Tilde expansion in autocomplete

- **WHEN** the user types `~/I` at the free-form path input
- **THEN** the autocomplete popup lists entries of the user's home directory whose names begin with `I`, surfaced as absolute paths (with `~` already expanded)

### Requirement: Per-repo workspace assignment after bulk-registration

After the repo-registration step (first-run or settings-panel) registers N>0 repos in a single invocation, the system SHALL prompt the user once whether to add them to workspaces. The prompt SHALL be a single `confirm` whose label reflects N (`"Add '<name>' to workspaces?"` when N=1; `"Add these N repos to workspaces?"` when N>1). On `no`, the flow continues without workspace changes.

On `yes`, the system SHALL walk two phases:

1. **Workspace prep (create-new loop)** — if any workspaces already exist, gate the loop behind a `"Create new workspaces first?"` confirm (default `no`); if none exist, enter the loop directly (the outer confirm already signalled intent and creation is the only path to a non-empty target pool). Inside the loop: prompt for a new workspace name (validated via `validate_workspace_name`, with duplicate-name pre-check), call `Config::create_workspace`, then ask `"Create another workspace?"` (default `no`) — break on `no`. This phase seeds the target pool of available workspaces.
2. **Per-repo assignment** — for each registered repo in registration order, render a `multiselect` labelled `"Workspaces for '<repo>'"` listing every workspace in the registry (existing + just-created) with no preselection and `.required(false)`. The user MAY pick zero, one, or many workspaces *for that specific repo*. Empty submissions SHALL leave that repo unassigned and proceed to the next repo. For each picked workspace, the system SHALL call `Config::add_members(workspace, &[repo])` exactly once.

After phase 2 completes, the system SHALL persist via a single `Config::save` so partial failure mid-loop cannot leave half-written membership on disk.

The target pool cannot be empty when phase 2 runs: phase 1 forces the create-new loop when no workspaces already exist, and the workspace-name prompt blocks until a valid name lands — so phase 2 is guaranteed at least one target to choose from.

The success log SHALL describe the per-repo assignments. When exactly one repo lands in exactly one workspace, the singular form `"added '<repo>' to '<ws>'"` SHALL be used. Otherwise, a multi-line `"workspace assignments:\n  <repo> → <ws1>, <ws2>\n  ..."` block SHALL list each assigned repo with its chosen workspaces, comma-separated. Repos with no picks SHALL be omitted from the block. When no repo received any picks, an `INFO` log `"no workspace assignments made"` SHALL replace the success block.

If zero repos were registered in this step, the workspace prompt SHALL NOT fire.

#### Scenario: Three repos routed to different existing workspaces

- **WHEN** the registry has existing workspaces `backend`, `frontend`, and `shared`; the user registers 3 repos `api`, `ui`, `lib`; confirms "Add these 3 repos to workspaces?"; declines "Create new workspaces first?"; the per-repo picker fires once per repo and the user ticks `backend` for `api`, `frontend` for `ui`, and both `backend` and `shared` for `lib`
- **THEN** `Config::add_members` is called once per (repo, workspace) pair (4 calls total), followed by a single `Config::save`; `backend.members` contains `["api", "lib"]`, `frontend.members` contains `["ui"]`, `shared.members` contains `["lib"]`; the success log is a multi-line block listing the three repos with their chosen workspaces

#### Scenario: Same workspace for every repo

- **WHEN** the user registers 3 repos `a`, `b`, `c`, confirms the outer prompt, declines create-new, and ticks the same single existing workspace `acme` for each of the three per-repo pickers
- **THEN** three `Config::add_members("acme", &[repo])` calls run followed by a single `Config::save`; `acme.members` contains `["a", "b", "c"]` (registration order); the success log is the multi-line block (not the singular form, because N>1)

#### Scenario: Single repo into single workspace uses singular wording

- **WHEN** the user registers exactly 1 repo, reaches the per-repo picker with one existing workspace `acme`, and ticks `acme`
- **THEN** the success log reads `"added '<name>' to 'acme'"` (singular both sides)

#### Scenario: Empty pick for one repo leaves that repo unassigned

- **WHEN** the user registers 3 repos, reaches per-repo assignment, picks workspaces for the first two, and submits an empty multiselect for the third
- **THEN** `Config::add_members` runs only for the first two repos; the third repo is registered but has no workspace membership; the success log's multi-line block lists only the first two repos with their chosen workspaces

#### Scenario: All repos empty produces info log no-op

- **WHEN** the user registers 4 repos, confirms the outer prompt, declines create-new, and submits empty multiselects for every per-repo picker
- **THEN** no `Config::add_members` is called; `Config::save` is still called (to persist any phase-1 workspace creations, none in this case); the cliclack output is an `INFO` log `"no workspace assignments made"`; exit code is `0`

#### Scenario: Create-new phase seeds the per-repo picker

- **WHEN** the user registers 2 repos `api`, `cli`; confirms the outer prompt; confirms "Create new workspaces first?"; creates two new workspaces `team-alpha`, `team-beta`; declines "Create another?"; ticks `team-alpha` for `api` and both `team-alpha` and `team-beta` for `cli`
- **THEN** `Config::create_workspace` runs for each new workspace; the per-repo pickers list `team-alpha` and `team-beta` as choices (plus any pre-existing workspaces, none in this scenario); `add_members` is called for the three (repo, workspace) pairs; a single `Config::save` persists everything; `team-alpha.members` is `["api", "cli"]` and `team-beta.members` is `["cli"]`

#### Scenario: No existing workspaces enters create-new loop directly

- **WHEN** the user registers 2 repos in a fresh config (zero workspaces); confirms the outer prompt; the create-new loop opens immediately without a gating confirm; creates a workspace `team`; declines "Create another?"; the per-repo pickers each show `team` as the only choice and the user ticks it for both
- **THEN** the existing-workspace gating confirm is skipped; one `create_workspace("team", None)` runs; two `add_members("team", &[repo])` calls run; a single `Config::save` persists everything; `team.members` is `["repo-a", "repo-b"]` in registration order

#### Scenario: Zero repos registered skips the workspace prompt entirely

- **WHEN** the user confirms "Register repos now?" → submits the repo multiselect with nothing selected → declines the "Register a repo at a custom path?" loop
- **THEN** zero repos are registered; the workspace prompt does NOT render; the flow proceeds to the summary

### Requirement: Manage-workspaces sub-flow adds repos in bulk via multiselect

The `Manage workspaces` sub-flow's `Create` and `Add members` actions SHALL allow adding any number of registered repos to the target workspace via a single `multiselect`, not one-at-a-time single-select. The intent is that creating a workspace and populating it with its repos are a single fluid step, not two trips through the settings panel.

Specifically:

- The `Create` action, after `Config::create_workspace` succeeds and is saved, SHALL render a `confirm` (default `yes`) asking whether to populate the new workspace now. On `yes`, the system SHALL run the same bulk-add subroutine as `Add members` against the newly created workspace. The `confirm` SHALL be skipped (and no add subroutine SHALL run) when zero repos are registered in the config.
- The `Add members` action SHALL render a `multiselect` listing all registered repos that are NOT already members of the target workspace, with no preselection. Submitting with zero selected SHALL be a valid no-op (no `Config::add_members` call, no `Config::save`). Submitting with N ≥ 1 selected SHALL invoke a single `Config::add_members(workspace, &picked)` followed by a single `Config::save`.

When all registered repos are already members of the target workspace (or no repos are registered at all), the system SHALL emit a `WARN` cliclack log explaining the no-op reason and return without rendering the multiselect.

Success logs SHALL use singular wording when exactly one repo lands (`"added '<repo>' to '<ws>'"`) and plural otherwise (`"added N repos to '<ws>'"`).

#### Scenario: Create flow chains into bulk-add when repos exist

- **WHEN** a config has 3 registered repos and the user runs `repograph init` → `Manage workspaces` → `Create` → enters a unique workspace name → confirms "Add repos to '<name>' now?" with the default `yes` → ticks 2 of the 3 repos in the multiselect
- **THEN** the saved config contains the new `[workspace.<name>]` with `members = ["repo-a", "repo-b"]` (in selection order); the success log reads "added 2 repos to '<name>'"

#### Scenario: Create flow skips the add prompt when no repos exist

- **WHEN** a config has zero registered repos and the user runs `Manage workspaces` → `Create` → enters a name
- **THEN** the workspace is created and saved with empty members; no "Add repos to ... now?" confirm renders; the flow returns to the settings panel

#### Scenario: Create flow with declined add leaves workspace empty

- **WHEN** a config has registered repos and the user creates a workspace then declines "Add repos to '<name>' now?"
- **THEN** the workspace is created with empty members; no multiselect renders; no further `Config::save` occurs

#### Scenario: Add members renders a multiselect filtered to non-members

- **WHEN** workspace `team` already contains `api`, the registry also contains `ui` and `cli`, and the user runs `Manage workspaces` → `Add members` → picks `team`
- **THEN** the multiselect renders with `ui` and `cli` listed (no preselection) and `api` excluded; ticking both submits `Config::add_members("team", &["ui", "cli"])` followed by `Config::save`; the success log reads "added 2 repos to 'team'"

#### Scenario: Add members with zero ticked is a no-op

- **WHEN** the user reaches the Add-members multiselect and submits with nothing ticked
- **THEN** `Config::add_members` is NOT called, `Config::save` is NOT called, the workspace's members are unchanged, and the flow returns to the settings panel

#### Scenario: Add members warns when all repos are already members

- **WHEN** every registered repo is already a member of the chosen workspace
- **THEN** a WARN-level cliclack log explains "all registered repos are already members — '<ws>' unchanged"; no multiselect renders; no `Config::save` occurs

#### Scenario: Add members warns when no repos are registered

- **WHEN** the config has zero registered repos and the user enters the Add-members action against any workspace
- **THEN** a WARN-level cliclack log explains "no repos registered yet — '<ws>' unchanged"; no multiselect renders

### Requirement: Settings-panel flow exposes update, register, manage, reset, cancel actions

When in settings-panel mode, the system SHALL render a top-level single-select with the actions: `Update agent selection`, `Register another repo`, `Manage workspaces`, `Reset everything`, `Cancel`. Each action SHALL be implemented as a sub-flow that composes the existing config primitives; `Cancel` SHALL exit `0` with no config writes.

The `Reset everything` action SHALL require an explicit confirm prompt (default = No) and SHALL, on confirmation, overwrite the config file with the byte-equivalent of `Config::default().save(dir)`.

#### Scenario: Update agent selection preserves repos and workspaces

- **WHEN** a config has `[agents] selected = ["claude-code"]`, two `[repo.*]` entries, and one `[workspace.*]` entry, and the user runs `repograph init` → `Update agent selection`, deselects `claude-code`, selects `agents-md`, and confirms
- **THEN** the saved config has `selected = ["agents-md"]`, the same two `[repo.*]` entries, and the same `[workspace.*]` entry unchanged

#### Scenario: Cancel exits without writing

- **WHEN** a config is present and the user runs `repograph init` → `Cancel`
- **THEN** the config file on disk is byte-identical to its pre-invocation state; exit code is `0`

#### Scenario: Reset everything requires confirmation and clears state

- **WHEN** a populated config is present and the user runs `repograph init` → `Reset everything` → `Yes`
- **THEN** the saved config is byte-equivalent to `Config::default()` serialized (no `[agents]`, no `[repo.*]`, no `[workspace.*]`); exit code is `0`

#### Scenario: Reset everything declined preserves state

- **WHEN** the user runs `repograph init` → `Reset everything` → `No` (default)
- **THEN** the config file is byte-identical to its pre-invocation state; the command returns to the settings-panel top-level select

### Requirement: Detection preselects agents based on host filesystem signals

In the first-run flow, the agent multiselect SHALL preselect entries based on the presence of well-known host paths. Detection signals SHALL be: `~/.claude/` or `~/.config/claude/` → `claude-code`; `~/.cursor/` → `cursor`; `~/.aider/` or `~/.aider.conf.yml` → `aider`; `~/.codeium/windsurf/` → `windsurf`; `~/.config/github-copilot/` → `copilot`. `agents-md` SHALL NEVER be auto-preselected; the user must explicitly opt in.

Detection SHALL be best-effort: missing or unreadable home directories SHALL produce no preselections (no error). The user SHALL always be able to deselect any preselected entry before confirming.

#### Scenario: All host signals present preselects all detected agents

- **WHEN** the user runs `repograph init` for the first time in an environment where `~/.claude/`, `~/.cursor/`, `~/.aider/`, `~/.codeium/windsurf/`, and `~/.config/github-copilot/` all exist
- **THEN** the multiselect renders with `claude-code`, `cursor`, `aider`, `windsurf`, and `copilot` preselected; `agents-md` is rendered but not preselected

#### Scenario: No host signals present preselects nothing

- **WHEN** the user runs `repograph init` in an environment where no detection signals exist
- **THEN** the multiselect renders all agents with no preselection; the user must select at least zero (empty selection is valid)

#### Scenario: Home directory unreadable degrades gracefully

- **WHEN** `dirs::home_dir()` returns `None` or the home directory is unreadable
- **THEN** detection silently yields no preselections; the multiselect still renders normally; no error is surfaced

#### Scenario: User can deselect a preselected agent

- **WHEN** detection preselects `claude-code` and the user deselects it before confirming
- **THEN** the saved config does not include `claude-code` in `selected`

### Requirement: Non-interactive variant accepts --agents and --no-prompt flags

The `init` command SHALL accept `--agents <list>` (comma-separated agent IDs) and `--no-prompt` flags. When `--no-prompt` is passed, the system SHALL skip all interactive UI, validate the `--agents` list against the agent registry, and write the resulting `[agents] selected = [...]` to config. `--no-prompt` without `--agents` SHALL exit `2` with a usage error naming both flags. `--agents` without `--no-prompt` in a TTY SHALL be valid and SHALL preselect the listed agents in the multiselect (the user can still adjust).

#### Scenario: Non-interactive happy path

- **WHEN** the user runs `repograph init --no-prompt --agents claude-code,cursor` against an empty config
- **THEN** the saved config contains exactly `[agents] selected = ["claude-code", "cursor"]`; no other sections; exit code is `0`; no cliclack UI is rendered

#### Scenario: Non-interactive overwrites existing agent selection

- **WHEN** a config has `[agents] selected = ["claude-code"]` and the user runs `repograph init --no-prompt --agents cursor`
- **THEN** the saved config has `selected = ["cursor"]`; any existing `[repo.*]` and `[workspace.*]` entries are preserved unchanged; exit code is `0`

#### Scenario: --no-prompt without --agents exits with usage error

- **WHEN** the user runs `repograph init --no-prompt`
- **THEN** the exit code is `2`, no config is written, and stderr names both `--no-prompt` and `--agents` and explains they must be used together

#### Scenario: --agents with unknown ID exits with usage error

- **WHEN** the user runs `repograph init --no-prompt --agents claude-code,bogus`
- **THEN** the exit code is `2`, no config is written, and stderr names `bogus` as not a recognized agent

#### Scenario: --agents in TTY mode preselects but allows adjustment

- **WHEN** stdout is a TTY, the config has no `[agents]`, and the user runs `repograph init --agents claude-code` without `--no-prompt`
- **THEN** the first-run multiselect renders with `claude-code` preselected; the user can deselect it before confirming

### Requirement: Non-TTY invocation without flags exits with a setup hint

When `repograph init` is invoked with stdout NOT a TTY and without `--no-prompt`, the system SHALL exit with code `2` and emit a message on stderr instructing the user to either run in an interactive shell or pass `--no-prompt --agents <list>`. No config writes SHALL occur on this path.

#### Scenario: Non-TTY init without flags exits 2

- **WHEN** the user runs `repograph init` with stdout redirected to a pipe and no `--no-prompt` flag
- **THEN** the exit code is `2`, stderr instructs the user to use `--no-prompt --agents` or run in an interactive shell, and the config file on disk is unchanged

### Requirement: Shared auto-prompt helper routes other commands through agent selection on first use

The binary crate SHALL expose a helper function `ensure_agents_configured(config, config_dir) -> Result<(), RepographError>` that any agent-consuming command can call before reading `[agents]`. The helper SHALL behave as follows:

1. If `config.agents()` returns `Some(_)` (section present, even if `selected` is empty): no-op, returns `Ok(())`.
2. If `config.agents()` returns `None` AND stdout is a TTY: render the same agent multiselect sub-flow as the first-run init (including detection-based preselection), mutate `config`, persist via `Config::save(config_dir)`, and return `Ok(())`.
3. If `config.agents()` returns `None` AND stdout is NOT a TTY: return a typed `RepographError::NeedsInit` variant (new) mapped to exit code `2`, with a message instructing the user to run `repograph init`.

#### Scenario: Helper is a no-op when [agents] is present

- **WHEN** a config has `[agents] selected = []` and the helper is invoked
- **THEN** no cliclack UI is rendered, no config write occurs, and the call returns `Ok(())`

#### Scenario: Helper prompts and persists when [agents] is missing in a TTY

- **WHEN** a config has no `[agents]` section, stdout is a TTY, and the helper is invoked
- **THEN** the agent multiselect renders, the user's selection is persisted to `<config-dir>/config.toml` under `[agents] selected = [...]`, and the in-memory `config` is mutated to reflect the same selection

#### Scenario: Helper errors with NeedsInit when [agents] is missing in non-TTY

- **WHEN** a config has no `[agents]` section, stdout is redirected to a pipe, and the helper is invoked
- **THEN** the helper returns `RepographError::NeedsInit`, the calling command exits with code `2`, stderr names `repograph init`, and no config write occurs

### Requirement: Output contract — stdout reserved for data, all interactive UI on stderr

The `init` command and the shared auto-prompt helper SHALL emit all cliclack output (intro, prompts, summary, error notes) to stderr. Stdout SHALL be untouched by the interactive flow. The non-interactive `--no-prompt` variant SHALL also produce no stdout output by default — confirmation of the saved selection SHALL emit only to stderr via `tracing::info!`.

#### Scenario: Interactive flow writes nothing to stdout

- **WHEN** the user runs `repograph init > out.txt` in a TTY (stderr remains attached) and completes the flow
- **THEN** the file `out.txt` is empty; the cliclack UI is visible on stderr

#### Scenario: Non-interactive variant writes nothing to stdout

- **WHEN** the user runs `repograph init --no-prompt --agents claude-code > out.txt`
- **THEN** the file `out.txt` is empty; success diagnostics appear on stderr

#### Scenario: Auto-prompt helper does not contaminate stdout of its caller

- **WHEN** a future command writes JSON to stdout and invokes `ensure_agents_configured` (config has no `[agents]`, stdout is a TTY) before its own output
- **THEN** the stdout stream contains only the JSON output of the calling command; cliclack UI from the helper appears on stderr

### Requirement: Exit codes follow the documented contract

The `init` command SHALL exit with the codes defined in `CLAUDE.md`: `0` success, `1` general failure (e.g. malformed existing TOML), `2` usage error (`--no-prompt` without `--agents`, unknown agent ID, non-TTY without flags, non-init command finding missing `[agents]` in non-TTY), `3` resource not found (e.g. composed `add` against a non-existent path), `4` permission denied on config write, `5` conflict (e.g. composed `add` against a name already registered, surfaced during in-flow re-prompt rather than as the exit code unless the user chooses to abort). No new exit codes SHALL be introduced.

#### Scenario: Successful init exits 0

- **WHEN** any successful init invocation completes (interactive or non-interactive)
- **THEN** the exit code is `0`

#### Scenario: Malformed existing TOML on load exits 1

- **WHEN** the user runs `repograph init` and the existing config file is not valid TOML
- **THEN** the exit code is `1` and stderr names the parse error

#### Scenario: Unknown agent ID in --agents exits 2

- **WHEN** the user runs `repograph init --no-prompt --agents bogus`
- **THEN** the exit code is `2`

#### Scenario: Permission denied on config write exits 4

- **WHEN** the config directory exists but is not writable, and the user completes an init flow that would write
- **THEN** the exit code is `4` and stderr names the permission failure
