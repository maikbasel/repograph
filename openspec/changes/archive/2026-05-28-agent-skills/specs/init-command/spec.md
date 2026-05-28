## ADDED Requirements

### Requirement: Init installs per-agent artifacts after agent selection

After agent selection completes — in both the interactive first-run flow and the non-interactive `--no-prompt` path — the `init` command SHALL invoke `repograph_core::agent_artifact::install_artifacts` with the resolved selection, scope, root, and force flag. The install step SHALL run for any `[agents]` invocation that produces a non-empty selection where at least one selected agent has an artifact writer (see the `agent-skills` capability). When no selected agent has a writer (or `selected = []`), the install step SHALL be skipped silently.

The install step SHALL run AFTER `Config::save` so that a failure to write artifacts does not roll back the agent-selection persistence. Per-agent failures SHALL be reported via `tracing::warn!` with the agent and error, and the command SHALL NOT exit non-zero solely because one artifact failed — the command's exit code SHALL reflect whether the *selection* persisted successfully, not whether every artifact was written.

The settings-panel "Update agent selection" sub-flow SHALL also invoke `install_artifacts` after the selection is persisted, applying the same rules. Other settings-panel actions (`Register another repo`, `Manage workspaces`, `Reset everything`) SHALL NOT trigger the install step.

#### Scenario: Interactive first run writes artifacts for each selected agent with a writer

- **WHEN** a user runs `repograph init` against an empty config in a TTY, selects `claude-code` and `agents-md`, and completes the flow
- **THEN** after `Config::save` the install layer is invoked; an artifact is written for each of the two agents at the matrix path for the chosen scope; the success log reflects the per-agent outcomes

#### Scenario: --no-prompt path writes artifacts

- **WHEN** the user runs `repograph init --no-prompt --agents claude-code --scope user`
- **THEN** after `Config::save` the install layer writes `~/.claude/skills/repograph/SKILL.md`; the command exits `0`

#### Scenario: Settings-panel "Update agent selection" refreshes artifacts

- **WHEN** the user runs `repograph init` (existing config with `[agents] selected = ["claude-code"]`) → `Update agent selection`, deselects `claude-code`, selects `agents-md` and `cursor`, and confirms
- **THEN** `Config::save` persists the new selection; the install layer is invoked for the new selection; artifacts for `agents-md` and `cursor` are written; no automatic removal occurs for the previously-selected `claude-code` artifact

#### Scenario: Empty selection skips the install step silently

- **WHEN** the user completes `init` with `selected = []` (explicitly opted out of agent docs)
- **THEN** the install layer is not invoked; no log lines about artifacts appear

#### Scenario: Selection containing only copilot skips the install step silently

- **WHEN** the user runs `repograph init --no-prompt --agents copilot`
- **THEN** `Config::save` persists the selection; the install layer is not invoked (because no selected agent has a writer); no scope-required validation fires (the prerequisite for that validation also depends on a writer-having agent being present)

#### Scenario: Per-agent failure does not abort the run or change the exit code

- **WHEN** the install layer reports `Failed` for one agent (e.g. permission denied on its target directory) and `Written` for another
- **THEN** a `warn!` log line names the failed agent and its error; the command exits `0` (selection persisted) and a stderr summary lists both outcomes

### Requirement: Init accepts --scope <user|project> flag with default user

The `init` command SHALL accept a `--scope <user|project>` argument (clap derive, optional). When omitted, the default SHALL be `Scope::User`. The flag SHALL be passed through to the artifact-install layer; it SHALL NOT affect agent-selection persistence (which always lives at the user-level config dir).

The flag SHALL be valid in both interactive and `--no-prompt` invocations. In the interactive flow, an explicit `--scope` flag SHALL skip the scope prompt; without the flag, the interactive flow SHALL prompt (see the scope-prompt requirement below).

#### Scenario: --scope user (default) writes to user-scope paths where supported

- **WHEN** the user runs `repograph init --no-prompt --agents claude-code` (no `--scope`)
- **THEN** the artifact lands at `<home>/.claude/skills/repograph/SKILL.md`; the default is applied

#### Scenario: --scope project writes to project-scope paths

- **WHEN** the user runs `repograph init --no-prompt --agents claude-code --scope project` from `/tmp/myproject`
- **THEN** the artifact lands at `/tmp/myproject/.claude/skills/repograph/SKILL.md`

#### Scenario: Invalid --scope value exits 2

- **WHEN** the user runs `repograph init --no-prompt --agents claude-code --scope bogus`
- **THEN** the exit code is `2`; clap renders the value-error message naming the valid values; no config write occurs

### Requirement: Init accepts --force flag that bypasses the delimiter check

The `init` command SHALL accept a `--force` boolean flag (default `false`). When passed, the flag SHALL be forwarded to the artifact-install layer, which SHALL overwrite each target file fresh (see the `agent-skills` capability's force-write requirement). The flag SHALL have no effect on agent-selection persistence.

#### Scenario: --force overwrites a user-authored AGENTS.md

- **WHEN** the project root contains an `AGENTS.md` with custom user prose (no delimited block) and the user runs `repograph init --no-prompt --agents agents-md --scope project --force`
- **THEN** the resulting `AGENTS.md` contains only the delimited repograph block; the prior user content is gone

#### Scenario: --force on identical content still rewrites

- **WHEN** the target file already has the exact delimited block and the install is invoked with `--force`
- **THEN** the file is rewritten; the per-agent log line reports `Written` (not `Unchanged`)

### Requirement: Interactive flow prompts for scope after agent selection when no flag is set

In the interactive first-run flow and the settings-panel "Update agent selection" sub-flow, the `init` command SHALL render a single-select prompt for `--scope` AFTER agent selection completes, IFF:

1. At least one selected agent has an artifact writer with a meaningful scope choice (i.e. its user-scope path differs from its project-scope path), AND
2. The user did NOT pass `--scope` on the command line, AND
3. Stdout is a TTY.

The prompt SHALL offer two options: `User (~)` and `Project (<cwd>)` with `User` as the default. The user's choice SHALL be passed to the install layer.

If every selected agent's user-scope path equals its project-scope path (e.g. selection is only project-only agents), the prompt SHALL be skipped and the install proceeds without a scope choice (the matrix's project-scope path is used for all).

#### Scenario: Scope prompt fires when claude-code is selected

- **WHEN** the interactive first-run user selects `claude-code` (with or without other agents) and did not pass `--scope`
- **THEN** after agent selection a single-select prompt renders with `User (~)` and `Project (<cwd>)`; the choice is applied to the install

#### Scenario: Scope prompt is skipped when only project-only agents are selected

- **WHEN** the interactive first-run user selects only `agents-md` and `aider`
- **THEN** no scope prompt renders; the install layer is invoked with the default `Scope::User` (which falls through to project for these agents anyway, per the agent-skills capability)

#### Scenario: --scope flag suppresses the prompt

- **WHEN** the user runs `repograph init --scope project` in a TTY, then completes the agent multiselect with `claude-code` selected
- **THEN** no scope prompt renders; the install uses `Scope::Project`

#### Scenario: Scope prompt does not fire in non-TTY

- **WHEN** stdout is not a TTY (and `--no-prompt` is not in effect, e.g. an error path that gets here)
- **THEN** no prompt renders; the scope-required validation (see modified non-interactive requirement) handles this case

## MODIFIED Requirements

### Requirement: Non-interactive variant accepts --agents and --no-prompt flags

The `init` command SHALL accept `--agents <list>` (comma-separated agent IDs), `--no-prompt`, and `--scope <user|project>` flags. When `--no-prompt` is passed, the system SHALL skip all interactive UI, validate the `--agents` list against the agent registry, write the resulting `[agents] selected = [...]` to config, and then invoke the artifact-install layer with the resolved `--scope` value.

`--no-prompt` without `--agents` SHALL exit `2` with a usage error naming both flags. `--agents` without `--no-prompt` in a TTY SHALL be valid and SHALL preselect the listed agents in the multiselect (the user can still adjust).

When `--no-prompt` is passed and at least one selected agent has an artifact writer with a meaningful scope choice (i.e. user-scope and project-scope paths differ), `--scope` SHALL be required. Invocations missing `--scope` in that case SHALL exit `2` with a usage error naming `--scope`, listing the affected agent(s), and explaining that scope must be explicit under `--no-prompt`. When every selected agent's paths are scope-invariant (project-only agents like `agents-md`, `aider`, `cursor`) or has no writer (`copilot`), `--scope` SHALL be optional and the install proceeds without consulting it.

#### Scenario: Non-interactive happy path

- **WHEN** the user runs `repograph init --no-prompt --agents claude-code,cursor --scope user` against an empty config
- **THEN** the saved config contains exactly `[agents] selected = ["claude-code", "cursor"]`; no other sections; the install layer writes the per-agent artifacts; exit code is `0`; no cliclack UI is rendered

#### Scenario: Non-interactive overwrites existing agent selection

- **WHEN** a config has `[agents] selected = ["claude-code"]` and the user runs `repograph init --no-prompt --agents cursor --scope project`
- **THEN** the saved config has `selected = ["cursor"]`; any existing `[repo.*]` and `[workspace.*]` entries are preserved unchanged; the cursor artifact is written; exit code is `0`

#### Scenario: --no-prompt without --agents exits with usage error

- **WHEN** the user runs `repograph init --no-prompt`
- **THEN** the exit code is `2`, no config is written, no artifact is installed, and stderr names both `--no-prompt` and `--agents` and explains they must be used together

#### Scenario: --agents with unknown ID exits with usage error

- **WHEN** the user runs `repograph init --no-prompt --agents claude-code,bogus --scope user`
- **THEN** the exit code is `2`, no config is written, no artifact is installed, and stderr names `bogus` as not a recognized agent

#### Scenario: --agents in TTY mode preselects but allows adjustment

- **WHEN** stdout is a TTY, the config has no `[agents]`, and the user runs `repograph init --agents claude-code` without `--no-prompt`
- **THEN** the first-run multiselect renders with `claude-code` preselected; the user can deselect it before confirming

#### Scenario: --no-prompt with a scope-bearing agent and no --scope exits 2

- **WHEN** the user runs `repograph init --no-prompt --agents claude-code` (no `--scope`)
- **THEN** the exit code is `2`, no config is written, no artifact is installed, and stderr names `--scope` and `claude-code` and explains that scope must be explicit under `--no-prompt`

#### Scenario: --no-prompt with only project-only agents does not require --scope

- **WHEN** the user runs `repograph init --no-prompt --agents agents-md,aider`
- **THEN** the exit code is `0`; the saved config contains the two agents; both artifacts are written to the project root (no scope choice was needed)

#### Scenario: --no-prompt with only copilot does not require --scope

- **WHEN** the user runs `repograph init --no-prompt --agents copilot`
- **THEN** the exit code is `0`; the saved config contains `copilot`; no artifact is installed (copilot has no writer); no scope-related error fires
