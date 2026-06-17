## MODIFIED Requirements

### Requirement: Doctor command runs a fixed catalog of read-only health checks against the config

The CLI SHALL accept a `repograph doctor` subcommand that loads the config and runs every check in the v1 catalog, producing a `DoctorReport` of findings. The catalog is the closed enum `repograph_core::doctor::Check`:

| Check                       | What it verifies                                                                                | Severity on fail |
|-----------------------------|-------------------------------------------------------------------------------------------------|------------------|
| `ConfigPresent`             | Config file exists at the resolved config dir                                                   | `error`          |
| `ConfigParse`               | Config file parses as TOML (only run if `ConfigPresent` passed)                                 | `error`          |
| `AgentsConfigured`          | `[agents]` section is present in the config                                                     | `warn`           |
| `ProjectsRootExists`        | `[settings].projects_root`, if set, points at an existing directory                             | `warn`           |
| `RepoPathExists`            | Per repo: the registered path exists on disk                                                    | `error`          |
| `RepoIsGitRepo`             | Per repo: the path opens as a `git2::Repository` (only run if `RepoPathExists` passed)          | `error`          |
| `WorkspaceMembersResolve`   | Per workspace: every `members[*]` name resolves to a registered repo                            | `warn`           |
| `AgentDocPresent`           | Per repo × per selected agent: at least one file matches the agent's pattern set                | `warn`           |
| `SkillArtifactFresh`        | Per selected agent × capability: the expected artifact exists and its version stamp matches the running binary | `warn`           |

`doctor` SHALL NOT mutate the config file or any agent artifact under any circumstance. `doctor` SHALL NOT perform network operations (no `git fetch`). The `SkillArtifactFresh` check SHALL be purely read-only: it resolves the expected artifact path for each selected agent and capability, reads the installed managed block's version stamp (if any), and compares it to the running binary's body version; it SHALL NOT write, create, or repair the artifact. Per-repo I/O (path existence check, `git2::Repository::open`, agent-doc pattern walk) SHALL be parallelized across the registered repo list via the existing `output::with_progress` helper. Workspace-level, config-level, and artifact-level checks SHALL run sequentially on the main thread.

Each check that passes for a given target SHALL emit a `Finding` with `severity = Severity::Ok` (not silently omitted), so consumers can audit which checks ran against which targets without consulting the catalog separately.

#### Scenario: Doctor on a clean config emits all-ok findings and exits 0

- **WHEN** the config has two registered repos (paths exist and open as git), one workspace with both repos as live members, `[agents].selected = ["claude-code"]` with each repo containing a `CLAUDE.md`, current-version skill artifacts installed, and `[settings].projects_root` pointing at an existing directory
- **THEN** every finding in the report has `severity = Severity::Ok`; `summary.ok > 0`, `summary.warn == 0`, `summary.error == 0`; exit code is `0`

#### Scenario: Doctor reports a missing repo path as an error finding

- **WHEN** the config registers `api` at a path that has been deleted from disk
- **THEN** the report contains a finding with `check = Check::RepoPathExists`, `severity = Severity::Error`, `target = "api"`, and a message naming the missing path; the dependent `Check::RepoIsGitRepo` finding for `api` is NOT emitted (skipped because its prerequisite failed); exit code is `1`

#### Scenario: Doctor reports a non-git path as an error finding

- **WHEN** the config registers `notes` at a path that exists but is not a git repo (no `.git/` directory)
- **THEN** the report contains an `ok` finding for `RepoPathExists` (the path does exist) and an `error` finding for `RepoIsGitRepo` for `notes`; exit code is `1`

#### Scenario: Doctor reports a dangling workspace member as a warning finding

- **WHEN** the config has a workspace `acme` with `members = ["api", "ghost"]` and only `api` is registered
- **THEN** the report contains a finding with `check = Check::WorkspaceMembersResolve`, `severity = Severity::Warn`, `target = "acme"`, and a message naming `ghost` as a dangling member; the `api` membership produces an `ok` finding for the same check; exit code is `0` (warnings do not gate)

#### Scenario: Doctor reports a missing agent doc as a warning finding

- **WHEN** the config has `[agents].selected = ["claude-code"]` and a registered repo `api` that contains no `CLAUDE.md`
- **THEN** the report contains a finding with `check = Check::AgentDocPresent`, `severity = Severity::Warn`, `target = "api / claude-code"`, and a message naming the absent pattern set; exit code is `0`

#### Scenario: Doctor reports a missing `[agents]` section as a warning

- **WHEN** the config exists and parses but has no `[agents]` section
- **THEN** the report contains a finding with `check = Check::AgentsConfigured`, `severity = Severity::Warn`, and a target naming the config file; the `AgentDocPresent` and `SkillArtifactFresh` checks are NOT run (no agents are configured to check against); exit code is `0`

#### Scenario: Doctor reports a missing skill artifact as a warning with a re-init hint

- **WHEN** the config has `[agents].selected = ["claude-code"]` and the expected `repograph-setup` skill artifact does not exist on disk
- **THEN** the report contains a finding with `check = Check::SkillArtifactFresh`, `severity = Severity::Warn`, a target naming the agent and capability, and a message recommending `run \`repograph init\``; exit code is `0`

#### Scenario: Doctor reports a stale skill artifact as a warning

- **WHEN** the installed consumer skill artifact carries an older version stamp than the running binary's body version
- **THEN** the report contains a `SkillArtifactFresh` finding with `severity = Severity::Warn` whose message names the installed and current versions and recommends `run \`repograph init\``; exit code is `0`

#### Scenario: Doctor does not mutate the config file or artifacts

- **WHEN** the user runs `repograph doctor` against a config file with mtime `T0` and skill artifacts with mtime `A0`
- **THEN** after the command exits, the config file's mtime is still `T0` and every skill artifact's mtime is still `A0`; no `.tmp` / backup files are left behind
