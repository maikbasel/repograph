## ADDED Requirements

### Requirement: Shared artifact body is the single source of truth

The system SHALL expose a single canonical artifact body as a `pub const &str` constant from `repograph_core::agent_artifact`. The constant SHALL contain all repograph-specific instructional prose (purpose statement, when-to-invoke triggers, command surface table, JSON schema cross-reference) and SHALL NOT contain per-agent frontmatter, headers, or wrappers. Per-agent writers SHALL wrap this constant; they SHALL NOT author body content independently.

The body's "Commands" section SHALL reference only the read-only command surface: `repograph context --json`, `repograph list --json`, `repograph status --json`, `repograph switch <name>`, `repograph doctor --json`. Mutating commands (`add`, `remove`, `workspace`, `init`) SHALL NOT appear in the Commands section. The body's "Things to avoid" section MAY name mutating commands as negative guidance (e.g. "do not run `repograph add` automatically; ask the user instead"), and SHALL include such negative guidance so the agent knows which surface it controls and which it must defer to the user on.

#### Scenario: Shared body is exported once

- **WHEN** `repograph_core::agent_artifact` is consumed
- **THEN** a single `pub const` body exists and is referenced by every per-agent writer; no writer duplicates body prose

#### Scenario: Every command name in the body is a real subcommand

- **WHEN** a test parses the body for `repograph <subcommand>` tokens and queries `<Cli as clap::CommandFactory>::command()` for each subcommand name
- **THEN** every name in the body resolves to a real `clap` subcommand; no dead references exist

#### Scenario: Mutating commands are excluded from the Commands section

- **WHEN** the body's `## Commands` section (from the heading up to the next `## ` heading) is searched for the strings `repograph add`, `repograph remove`, `repograph workspace`, `repograph init`
- **THEN** none appear; the Commands section covers only read-only flows

#### Scenario: Body warns against running mutating commands automatically

- **WHEN** the body is searched for the don't-mutate guidance string
- **THEN** the "Things to avoid" section contains an explicit reminder that the agent must not invoke mutating registry commands on its own initiative

### Requirement: Per-agent artifact path matrix is a fixed closed mapping

The system SHALL define a fixed mapping from `(AgentId, Scope)` to a target file path. The v1 matrix SHALL be exactly:

| Agent ID      | User scope                                       | Project scope                                    |
|---------------|--------------------------------------------------|--------------------------------------------------|
| `claude-code` | `<home>/.claude/skills/repograph/SKILL.md`       | `<cwd>/.claude/skills/repograph/SKILL.md`        |
| `agents-md`   | `<cwd>/AGENTS.md` (no user scope; see below)     | `<cwd>/AGENTS.md`                                |
| `cursor`      | `<cwd>/.cursor/rules/repograph.mdc` (no user scope; see below) | `<cwd>/.cursor/rules/repograph.mdc`     |
| `aider`       | `<cwd>/CONVENTIONS.md` (no user scope; see below)| `<cwd>/CONVENTIONS.md`                           |
| `windsurf`    | `<home>/.codeium/windsurf/memories/repograph.md` | `<cwd>/.windsurfrules`                           |
| `copilot`     | (none — deferred)                                | (none — deferred)                                |

Agents whose path lacks a user-scope variant SHALL silently fall through to the project-scope path when the caller passes `Scope::User`. The mapping SHALL NOT be user-extensible; new agents are added by code change.

#### Scenario: Each (agent, scope) pair resolves to the matrix path

- **WHEN** the path resolver is queried with each entry in the v1 matrix in turn
- **THEN** the returned path matches the table above; `<home>` resolves via `dirs::home_dir()` and `<cwd>` via `std::env::current_dir()`

#### Scenario: Project-only agents fall through under scope=user

- **WHEN** the resolver is called with `(AgentId::AgentsMd, Scope::User)` or `(AgentId::Aider, Scope::User)` or `(AgentId::Cursor, Scope::User)`
- **THEN** the returned path is the project-scope path and an `info!` log line names the agent and explains the fall-through

#### Scenario: Copilot is skipped in v1

- **WHEN** the resolver is called with `(AgentId::Copilot, _)`
- **THEN** the install layer returns `ArtifactResult::Skipped { reason: "copilot v1 deferred" }` with no file write attempted

### Requirement: Per-agent writers produce native-format output

The system SHALL define a writer per supported agent ID that takes the shared body and produces the on-disk representation. The writer SHALL determine native format:

- `claude-code` (`SKILL.md`): YAML frontmatter block at file head with `name: repograph` and `description: <one-line summary>`, followed by the wrapped body.
- `agents-md` (`AGENTS.md`): markdown with a top-level `# repograph` heading inside the managed section.
- `cursor` (`.cursor/rules/repograph.mdc`): MDC frontmatter block at file head with `description: <one-line summary>` and `globs: []`, followed by the wrapped body.
- `aider` (`CONVENTIONS.md`): plain markdown with a top-level `# repograph` heading inside the managed section.
- `windsurf` (`.windsurfrules` for project, `<home>/.codeium/windsurf/memories/repograph.md` for user): plain markdown with a top-level `# repograph` heading inside the managed section.

The "one-line summary" SHALL be a single `const &str` shared with the body and SHALL describe repograph as "cross-repo context for AI agents."

#### Scenario: claude-code writer emits SKILL.md with valid frontmatter

- **WHEN** the claude-code writer is invoked
- **THEN** the output begins with `---\nname: repograph\ndescription: <summary>\n---\n` and contains the shared body wrapped in the managed-section delimiters

#### Scenario: cursor writer emits .mdc with valid MDC frontmatter

- **WHEN** the cursor writer is invoked
- **THEN** the output begins with the MDC frontmatter block (`---\ndescription: <summary>\nglobs: []\n---\n`) and contains the shared body wrapped in the managed-section delimiters

#### Scenario: agents-md / aider / windsurf writers emit plain markdown

- **WHEN** any of those writers is invoked
- **THEN** the output contains no YAML frontmatter; the content is the managed-section delimiters wrapping the shared body, with a `# repograph` heading as the first line inside the delimiters

### Requirement: Managed-section delimiter contract makes installation idempotent

The system SHALL wrap repograph-managed body content in a delimiter pair using HTML comment syntax: `<!-- repograph:begin -->` and `<!-- repograph:end -->`. The install algorithm SHALL be:

1. If the target file does not exist: write `<delimiter-begin>\n<body>\n<delimiter-end>\n`.
2. If the target file exists and contains the delimiter pair: extract the current delimited content; if byte-identical to the new body, return `ArtifactResult::Unchanged`; otherwise rewrite only the delimited region, preserving every byte outside it, and return `ArtifactResult::Written`.
3. If the target file exists and does NOT contain the delimiter pair: append a single newline (if the file does not end with one) plus `<delimiter-begin>\n<body>\n<delimiter-end>\n` and return `ArtifactResult::Written`.

The delimiters SHALL be byte-stable across runs. The `<body>` between delimiters SHALL be deterministic for a given (agent, scope, body version) tuple — no timestamps, no per-host strings.

#### Scenario: Fresh install writes a delimited block

- **WHEN** the target file does not exist and the installer runs for `agents-md`
- **THEN** the file is created with exactly `<delimiter-begin>\n<body>\n<delimiter-end>\n` content; the install returns `Written`

#### Scenario: Re-run with identical body is a no-op

- **WHEN** the target file contains a delimited block whose body is byte-identical to the new body
- **THEN** the file is not rewritten (no I/O write call); the install returns `Unchanged`

#### Scenario: Re-run with body version bump rewrites only the delimited region

- **WHEN** the target file contains `user-prefix\n<delimiter-begin>\nOLD\n<delimiter-end>\nuser-suffix\n` and the new body is `NEW`
- **THEN** the file becomes `user-prefix\n<delimiter-begin>\nNEW\n<delimiter-end>\nuser-suffix\n`; the install returns `Written`

#### Scenario: Existing user file without delimiters gets the block appended

- **WHEN** the target `AGENTS.md` contains `# Existing user prose\n` and no delimiter pair
- **THEN** the resulting file is `# Existing user prose\n\n<delimiter-begin>\n<body>\n<delimiter-end>\n`; the install returns `Written`

### Requirement: Force flag bypasses the delimiter check and overwrites the file

When the caller passes `force = true`, the install algorithm SHALL skip the existence and delimiter checks and write the file fresh with exactly `<delimiter-begin>\n<body>\n<delimiter-end>\n` content, discarding any prior file contents.

#### Scenario: Force overwrites user-authored content

- **WHEN** the target `AGENTS.md` contains custom user prose with no repograph block, and the install is invoked with `force = true`
- **THEN** the file is replaced with the delimited block only; prior user content is gone; the install returns `Written`

#### Scenario: Force on identical file still rewrites (not Unchanged)

- **WHEN** the target file already has the exact delimited block and the install is invoked with `force = true`
- **THEN** the file is rewritten with the same content; the install returns `Written` (not `Unchanged`)

### Requirement: Install returns a typed result per agent

The system SHALL define an enum `ArtifactResult` with at least the variants `Written { path: PathBuf }`, `Unchanged { path: PathBuf }`, `Skipped { agent: AgentId, reason: String }`, and `Failed { agent: AgentId, error: RepographError }`. The `install_artifacts` entry point SHALL return `Vec<ArtifactResult>` — one entry per agent in the input selection, in selection order. Errors for individual agents SHALL NOT abort the run; they SHALL be captured as `Failed` and the run SHALL proceed for the remaining agents.

#### Scenario: Mixed outcomes are reported per agent

- **WHEN** the caller installs for `[claude-code, agents-md, copilot]` and the claude-code target directory is not writable
- **THEN** the result vector contains three entries: `Failed { claude-code, ... }`, `Written { .../AGENTS.md }` (or `Unchanged`), `Skipped { copilot, "v1 deferred" }`; the install does not return early

#### Scenario: Order matches selection order

- **WHEN** the selection is `[agents-md, claude-code]`
- **THEN** the result vector's first entry corresponds to `agents-md` and the second to `claude-code`

### Requirement: Output contract — install diagnostics emit to stderr only

The system SHALL emit all install-time diagnostics (success log per agent, fall-through warning for project-only agents under scope=user, skip log for copilot, per-error log on `Failed`) via `tracing` to stderr. The install layer SHALL NOT write to stdout under any circumstance.

#### Scenario: Install does not contaminate stdout

- **WHEN** the caller redirects stdout to a file and runs an install that produces `Written`, `Unchanged`, `Skipped`, and `Failed` outcomes
- **THEN** the stdout file is empty; all log lines appear on stderr

### Requirement: Agent registry exposes which agents have artifact writers

The system SHALL expose a query `AgentId::has_artifact_writer(&self) -> bool` (or equivalent) that returns true for every agent ID with a writer in this change and false for `Copilot` (deferred). This SHALL allow callers (e.g. the init command) to skip the prompt step entirely if no selected agent has a writer.

#### Scenario: Copilot reports no writer

- **WHEN** `AgentId::Copilot.has_artifact_writer()` is queried
- **THEN** the result is `false`

#### Scenario: All other v1 agents report a writer

- **WHEN** the same query runs for each of `claude-code`, `agents-md`, `cursor`, `aider`, `windsurf`
- **THEN** every result is `true`
