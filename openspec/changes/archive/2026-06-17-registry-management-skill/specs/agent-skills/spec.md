## MODIFIED Requirements

### Requirement: Shared artifact body is the single source of truth

The system SHALL expose the canonical artifact bodies as `pub const &str` constants from `repograph_core::agent_artifact`: a **consumer** body (read-only surface) and a **setup** body (mutating surface), selected by a `Capability` value. Each constant SHALL contain all repograph-specific instructional prose for its capability (purpose statement, when-to-invoke triggers, command surface table, JSON schema cross-reference) and SHALL NOT contain per-agent frontmatter, headers, or wrappers. Per-agent writers SHALL wrap these constants; they SHALL NOT author body content independently.

The **consumer** body's "Commands" section SHALL reference only the read-only command surface: `repograph context --json`, `repograph list --json`, `repograph status --json`, `repograph switch <name>`, `repograph doctor --json`. Mutating commands (`add`, `remove`, `edit`, `workspace`, `init`) SHALL NOT appear in the consumer Commands section. The consumer body's "Things to avoid" section SHALL include negative guidance that the agent must not invoke mutating registry commands on its own initiative, and SHALL delegate those operations to the setup skill (`repograph-setup`) rather than dead-ending at "ask the user."

The **setup** body's "Commands" section SHALL cover the mutating surface (`add`, `remove`, `edit`, `workspace create/add/remove/rm`) and SHALL instruct a plan → confirm → execute → verify workflow: resolve and validate inputs, present the concrete plan to the user, mutate only on confirmation, then verify via the command's `--json` confirmation envelope.

#### Scenario: Both bodies are exported once

- **WHEN** `repograph_core::agent_artifact` is consumed
- **THEN** a `pub const` consumer body and a `pub const` setup body each exist and are referenced by every per-agent writer for their capability; no writer duplicates body prose

#### Scenario: Every command name in each body is a real subcommand

- **WHEN** a test parses each body for `repograph <subcommand>` tokens and queries `<Cli as clap::CommandFactory>::command()` for each subcommand name
- **THEN** every name resolves to a real `clap` subcommand; no dead references exist

#### Scenario: Mutating commands are excluded from the consumer Commands section

- **WHEN** the consumer body's `## Commands` section (from the heading up to the next `## ` heading) is searched for `repograph add`, `repograph remove`, `repograph edit`, `repograph workspace`, `repograph init`
- **THEN** none appear; the consumer Commands section covers only read-only flows

#### Scenario: Consumer body delegates mutation to the setup skill

- **WHEN** the consumer body's "Things to avoid" section is searched for the don't-mutate guidance
- **THEN** it contains an explicit reminder not to invoke mutating registry commands automatically AND names the `repograph-setup` skill as the surface that handles registration, grouping, and edits

#### Scenario: Setup body covers the mutating surface with a confirm-before-write workflow

- **WHEN** the setup body's `## Commands` section is inspected
- **THEN** it references `add`, `remove`, `edit`, and `workspace` subcommands, and the body instructs the agent to present a plan and obtain user confirmation before running any mutation and to verify via the `--json` confirmation envelope

### Requirement: Managed-section delimiter contract makes installation idempotent

The system SHALL wrap repograph-managed body content in a delimiter pair using HTML comment syntax that carries a body version stamp: `<!-- repograph:begin v<N> -->` and `<!-- repograph:end -->`, where `<N>` is the current artifact body version. The install algorithm SHALL be:

1. If the target file does not exist: write `<delimiter-begin>\n<body>\n<delimiter-end>\n`.
2. If the target file exists and contains a `repograph:begin`/`repograph:end` pair (any version): extract the current delimited content; if byte-identical to the new block (including the version stamp), return `ArtifactResult::Unchanged`; otherwise rewrite only the delimited region, preserving every byte outside it, and return `ArtifactResult::Written`.
3. If the target file exists and does NOT contain the delimiter pair: append a single newline (if the file does not end with one) plus `<delimiter-begin>\n<body>\n<delimiter-end>\n` and return `ArtifactResult::Written`.

The delimiter detection SHALL match any version stamp so an older-version block is recognized and rewritten in place rather than appended. The `<body>` between delimiters SHALL be deterministic for a given (agent, capability, scope, body version) tuple — no timestamps, no per-host strings. The version stamp SHALL be machine-readable so a consumer (e.g. `doctor`) can compare an installed block's version against the running binary's.

#### Scenario: Fresh install writes a version-stamped delimited block

- **WHEN** the target file does not exist and the installer runs for `agents-md`
- **THEN** the file is created with `<!-- repograph:begin v<N> -->\n<body>\n<!-- repograph:end -->\n` for the current version `<N>`; the install returns `Written`

#### Scenario: Re-run with identical version and body is a no-op

- **WHEN** the target file contains a delimited block whose version stamp and body are byte-identical to the new block
- **THEN** the file is not rewritten (no I/O write call); the install returns `Unchanged`

#### Scenario: Older-version block is rewritten in place

- **WHEN** the target file contains `user-prefix\n<!-- repograph:begin v1 -->\nOLD\n<!-- repograph:end -->\nuser-suffix\n` and the current version is `v2` with body `NEW`
- **THEN** the file becomes `user-prefix\n<!-- repograph:begin v2 -->\nNEW\n<!-- repograph:end -->\nuser-suffix\n`; bytes outside the delimiters are preserved; the install returns `Written`

#### Scenario: Existing user file without delimiters gets the block appended

- **WHEN** the target `AGENTS.md` contains `# Existing user prose\n` and no delimiter pair
- **THEN** the resulting file is `# Existing user prose\n\n<!-- repograph:begin v<N> -->\n<body>\n<!-- repograph:end -->\n`; the install returns `Written`

### Requirement: Install returns a typed result per agent

The system SHALL define an enum `ArtifactResult` whose variants carry the `Capability` they pertain to in addition to the existing fields: at least `Written { capability: Capability, path: PathBuf }`, `Unchanged { capability: Capability, path: PathBuf }`, `Skipped { agent: AgentId, reason: String }`, and `Failed { agent: AgentId, capability: Capability, error: RepographError }`. The `install_artifacts` entry point SHALL return `Vec<ArtifactResult>` — one entry per (agent, capability) artifact actually targeted, in selection order then capability order (Consumer before Setup). Wholly-owned-file agents SHALL produce two entries (one per capability); flat-file agents SHALL produce a single entry whose block contains both capabilities inlined. Errors for individual artifacts SHALL NOT abort the run; they SHALL be captured as `Failed` and the run SHALL proceed for the remaining artifacts.

#### Scenario: Wholly-owned-file agent yields one result per capability

- **WHEN** the caller installs for `[claude-code]`
- **THEN** the result vector contains a `Consumer` entry for `skills/repograph/SKILL.md` and a `Setup` entry for `skills/repograph-setup/SKILL.md`, in that order

#### Scenario: Flat-file agent yields a single combined result

- **WHEN** the caller installs for `[agents-md]`
- **THEN** the result vector contains exactly one entry for `AGENTS.md` whose written block contains both the consumer and setup bodies inlined

#### Scenario: Mixed outcomes are reported per artifact

- **WHEN** the caller installs for `[claude-code, agents-md, copilot]` and the claude-code target directory is not writable
- **THEN** the result vector contains the claude-code artifacts as `Failed`, the `agents-md` artifact as `Written`/`Unchanged`, and `Skipped { copilot, … }`; the install does not return early

## ADDED Requirements

### Requirement: Setup-capability skill is generated alongside the consumer skill

The system SHALL emit a second `repograph-setup` capability per selected agent, governed by the agent's file model as determined by the existing `wholly_owned_file(agent)` predicate:

- **Wholly-owned-file agents** (`claude-code`, `cursor`) SHALL receive a **discrete** second artifact at a setup-specific path: `<root>/.claude/skills/repograph-setup/SKILL.md` for `claude-code` and `<root>/.cursor/rules/repograph-setup.mdc` for `cursor`, where `<root>` follows the same user/project scope resolution as the consumer artifact. The setup artifact SHALL carry its own frontmatter `name: repograph-setup` (Claude) / `description` (Cursor) reflecting the setup `SUMMARY`.
- **Flat-file agents** (`agents-md`, `aider`, `windsurf`) SHALL NOT receive a second file; instead `render_artifact` SHALL produce a single managed block whose content is the consumer body followed by the setup body, written to the agent's existing single path.

The setup `SUMMARY` SHALL be a distinct `const &str` whose trigger phrasing covers registering repos, grouping repos into workspaces, and updating/editing existing registry entries — disjoint from the consumer `SUMMARY`'s read/resolve phrasing.

#### Scenario: claude-code setup skill resolves to a discrete path

- **WHEN** the path resolver is queried for `(AgentId::ClaudeCode, Capability::Setup, Scope::User)`
- **THEN** it returns `<home>/.claude/skills/repograph-setup/SKILL.md`, distinct from the consumer `skills/repograph/SKILL.md`

#### Scenario: cursor setup skill resolves to a discrete .mdc

- **WHEN** the path resolver is queried for `(AgentId::Cursor, Capability::Setup, _)`
- **THEN** it returns `<cwd>/.cursor/rules/repograph-setup.mdc`, distinct from the consumer `.cursor/rules/repograph.mdc`

#### Scenario: Flat-file agent inlines both capabilities into one file

- **WHEN** `render_artifact` is invoked for `agents-md`
- **THEN** the produced managed block contains the consumer body and the setup body concatenated, and the resolver returns the single `AGENTS.md` path for both capabilities (no second file is written)

#### Scenario: Setup summary is distinct from the consumer summary

- **WHEN** the setup `SUMMARY` and consumer `SUMMARY` are compared
- **THEN** the setup summary's triggers name registering/grouping/updating repos and the consumer summary's do not; the two strings are not equal
