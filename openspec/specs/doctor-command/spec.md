# doctor-command Specification

## Purpose

The `doctor-command` capability is repograph's read-only health check: it loads the config and runs a closed catalog of checks across every registered repo, workspace, and the agent configuration, surfacing the drift that accumulates over time (deleted paths, dangling workspace members, repos that lost their `.git/`, missing agent docs that would silently produce empty `context` sections). It defines the v1 `Check` catalog, the `Finding` / `Severity` / `DoctorReport` data model, the stable versioned JSON envelope (`schema_version: 1`) for agent consumption, the TTY `comfy-table` rendering with colorized severities, the exit-code mapping (`0` clean or warn-only; `1` any error; `4` config permission-denied), the strict read-only and zero-network contract, and the composition rule that prevents `doctor.rs` from duplicating git-open or agent-pattern logic owned by the `git` and `context` modules. The capability composes `registry-core`, `workspace-support`, `init-command`, and `context-command` without modifying them.
## Requirements
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

### Requirement: Doctor JSON envelope is stable and versioned

When `--json` is passed or stdout is NOT a TTY, the command SHALL emit a single-line JSON object to stdout with this shape:

```json
{
  "schema_version": 1,
  "generated_at": "<RFC 3339 UTC timestamp>",
  "checks": [
    {
      "check": "<Check enum name>",
      "severity": "ok" | "warn" | "error",
      "target": "<string>",
      "message": "<string>"
    },
    ...
  ],
  "summary": { "ok": <u32>, "warn": <u32>, "error": <u32>, "total": <u32> }
}
```

`schema_version` SHALL be the integer `1` for this version of the contract. The schema SHALL be additive-only at version `1`; any breaking change SHALL bump the version.

The `checks` array SHALL be sorted by `(severity DESC, check ASC, target ASC)` where severity order is `error > warn > ok`. `check` SHALL be the `Check` enum variant name in PascalCase (e.g. `"RepoPathExists"`). `severity` SHALL be the lowercase variant name (`"ok"`, `"warn"`, `"error"`). `summary.total` SHALL equal `summary.ok + summary.warn + summary.error`.

The JSON output SHALL be a single-line emission (no trailing newline) suitable for piping into `jq`. Stdout SHALL contain only this JSON payload; all diagnostics SHALL go to stderr via `tracing`.

#### Scenario: JSON payload validates as a single JSON object

- **WHEN** the user runs `repograph doctor --json` against any config and pipes the output through `jq '.schema_version'`
- **THEN** `jq` succeeds and prints `1`; stdout is parseable as a single JSON object

#### Scenario: JSON checks are sorted by severity desc, then check asc, then target asc

- **WHEN** the user runs `repograph doctor --json` against a config that produces one `error` finding, two `warn` findings, and three `ok` findings
- **THEN** the `checks` array's first element has `severity == "error"`, followed by both `warn` entries, followed by all three `ok` entries; ties on severity are broken by check-name ascending, then target ascending

#### Scenario: Summary totals match the checks array

- **WHEN** the user runs `repograph doctor --json` against any config
- **THEN** `summary.ok`, `summary.warn`, `summary.error` equal the counts of each severity in `checks`; `summary.total` equals their sum and equals `checks | length`

#### Scenario: Non-TTY without --json still emits JSON

- **WHEN** the user runs `repograph doctor > out.json` (stdout redirected to a file)
- **THEN** `out.json` parses as the same JSON object as if `--json` had been passed explicitly; exit code follows the doctor exit-code contract

#### Scenario: Stdout contains only the payload

- **WHEN** the user runs `repograph doctor --json 2>/dev/null` against any valid config
- **THEN** stdout contains exactly the JSON payload (no leading or trailing log lines, banners, or spinner artifacts)

### Requirement: Doctor TTY output renders the same data as a comfy-table summary

When stdout is a TTY and `--json` is NOT passed, the command SHALL emit a `comfy-table` (preset `UTF8_FULL`) on stdout with columns `Severity | Check | Target | Message`. Rows SHALL be sorted in the same order as the JSON `checks` array (severity desc, check asc, target asc). The `Severity` column SHALL be colorized via `console::Style`: `error` red, `warn` yellow, `ok` green. Coloring SHALL no-op when stdout is not a true TTY (handled by `console` automatically).

A single footer line SHALL follow the table on stdout: `<N> ok · <M> warn · <K> error` with the same color treatment per count. When `summary.error > 0`, a hint SHALL be emitted to stderr (not stdout) suggesting `run \`repograph doctor --json | jq\` for machine-readable detail`.

Stdout SHALL contain only the table and footer. All diagnostics SHALL go to stderr.

#### Scenario: TTY default emits a comfy-table with the documented columns

- **WHEN** stdout is a TTY and the user runs `repograph doctor` against any non-empty config
- **THEN** stdout begins with a `comfy-table` rendering whose header row contains `Severity`, `Check`, `Target`, and `Message` in that order; each data row corresponds to one finding; the table is followed by a single footer line of the form `<N> ok · <M> warn · <K> error`

#### Scenario: TTY rendering preserves the stdout-only contract

- **WHEN** the user runs `repograph doctor 2>err.log` in a TTY
- **THEN** stdout contains the table and footer with no log lines interleaved; `err.log` contains the `tracing` diagnostics; stdout is byte-identical to running the command without stderr redirection (modulo color escape sequences if the terminal supports them)

#### Scenario: Error-only hint appears on stderr, never stdout

- **WHEN** the user runs `repograph doctor` against a config that produces at least one `error` finding
- **THEN** stderr contains the hint line referencing `repograph doctor --json | jq`; stdout contains only the table and footer with no hint

### Requirement: Doctor exit codes — 0 on no-error, 1 on any error, 4 on config permission-denied

`doctor` SHALL exit with code `0` when every finding has severity `ok` or `warn` (warnings do not gate). `doctor` SHALL exit with code `1` when at least one finding has severity `error`, including the synthetic finding produced when `Check::ConfigPresent` fails. `doctor` SHALL exit with code `4` when the config file exists but cannot be read due to permission denied (`std::io::ErrorKind::PermissionDenied`); in this case, no JSON envelope or table is emitted (stdout is empty) and the error surfaces through the standard `RepographError::Io` path.

#### Scenario: Clean config exits 0

- **WHEN** the user runs `repograph doctor` against a config where every check passes
- **THEN** exit code is `0`

#### Scenario: Warning-only config exits 0

- **WHEN** the user runs `repograph doctor` against a config that produces only `warn` findings (e.g. dangling workspace member, missing agent doc)
- **THEN** exit code is `0`; the report is emitted in full on stdout

#### Scenario: Any error finding exits 1

- **WHEN** the user runs `repograph doctor` against a config that produces at least one `error` finding (e.g. a missing repo path)
- **THEN** exit code is `1`; the report is emitted in full on stdout

#### Scenario: Missing config file exits 1, not 3

- **WHEN** the user runs `repograph doctor` and the config file does not exist
- **THEN** exit code is `1`; the report contains a single `error` finding for `Check::ConfigPresent` naming the expected config path; stdout still emits the report (this command is diagnostic — it reports absence rather than aborting on it)

#### Scenario: Config permission-denied exits 4

- **WHEN** the user runs `repograph doctor` and the config file exists but the process cannot read it (filesystem permission denied)
- **THEN** exit code is `4`; stdout is empty; stderr names the permission failure

### Requirement: Doctor reuses core helpers — no duplication of git-open or agent-pattern logic

The `doctor` module in `repograph-core` SHALL call `crate::git::validate_git_repo` (or equivalent existing helper) for `Check::RepoIsGitRepo` and SHALL call `crate::context::resolve_agent_docs` for `Check::AgentDocPresent`. No `git2::Repository::open` invocation and no `globset` pattern compilation SHALL appear in `doctor.rs` directly — those concerns are owned by the existing `git` and `context` modules and the doctor only composes them.

For `Check::AgentDocPresent`, the doctor SHALL inspect only the `files.is_empty()` flag of each resolved `AgentDoc`; it SHALL NOT read file contents (and SHALL NOT incur the cost of doing so).

#### Scenario: Doctor's per-repo git check uses the core git helper

- **WHEN** a registered repo path exists but is not a git repository
- **THEN** the `Check::RepoIsGitRepo` finding's `message` contains the substring or wording produced by `crate::git::validate_git_repo`'s error path (consistent with the wording elsewhere in the binary when the same condition is detected)

#### Scenario: Doctor's agent-doc presence check matches context's resolution

- **WHEN** a registered repo contains files that `repograph context --json` includes in its `agent_docs[*].files` array for a given agent
- **THEN** `doctor`'s `Check::AgentDocPresent` finding for that `repo / agent` pair has `severity = Severity::Ok`; conversely, when `repograph context --json` reports empty `files` for that pair, `doctor` reports `severity = Severity::Warn`

### Requirement: Tracing logs entry, success, and error for doctor

The `doctor` command's `run` function SHALL emit `tracing` logs at three points per the project logging rule:

- **Entry (`debug`)**: command name and `json` flag value.
- **Success (`info`)**: structured fields for `ok`, `warn`, `error` counts and `total`.
- **Error (`error`)**: the error itself plus context (e.g. config path on permission denied).

Per-finding `warn!` / `error!` lines SHALL NOT be emitted (they would duplicate the table/JSON output and create stderr noise); the structured payload on stdout is the single source of truth for the findings.

#### Scenario: Successful invocation emits debug entry and info success on stderr

- **WHEN** the user runs `repograph doctor --json` with `RUST_LOG=repograph=debug`
- **THEN** stderr contains a `DEBUG` line on entry, and an `INFO` line on success with structured fields for `ok`, `warn`, `error`, and `total`; no per-finding `WARN` or `ERROR` lines appear in stderr

### Requirement: README documents the doctor command surface and check catalog

The project `README.md` SHALL document the `repograph doctor` subcommand under its command table and SHALL include a "Doctor" subsection covering:

- The full check catalog (one row per `Check` variant with what it verifies and its failure severity).
- The JSON envelope example showing `schema_version`, `generated_at`, `checks`, and `summary` fields.
- The exit-code mapping (`0` clean / warn-only; `1` any error; `4` config permission-denied).
- A note that `doctor` is read-only and zero-network.

#### Scenario: README contains a doctor section

- **WHEN** a reader opens `README.md` and searches for `repograph doctor`
- **THEN** they find a section with the check catalog table, the JSON envelope shape, and the exit-code mapping; the section explicitly states the command is read-only and performs no network operations

### Requirement: Doctor reports search index health

`repograph doctor` SHALL include a check reporting the state of the cross-repo search index. The check SHALL report `ok` when the index exists and every registered repo's indexed commit matches its current HEAD, `warn` when the index is missing or stale for one or more repos, and SHALL never panic when the index database is absent or unreadable. The check SHALL identify which repos are stale so the user knows to run `repograph index`.

#### Scenario: Index present and current

- **WHEN** the user runs `repograph doctor` after indexing with no subsequent commits
- **THEN** the index check reports `ok`

#### Scenario: Index missing

- **WHEN** the user runs `repograph doctor` before ever building an index
- **THEN** the index check reports `warn` indicating the index has not been built
- **AND** doctor does not panic

#### Scenario: Index stale relative to HEAD

- **WHEN** a registered repo has new commits since it was last indexed
- **THEN** the index check reports `warn` and names the stale repo

