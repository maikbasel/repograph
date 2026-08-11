## MODIFIED Requirements

### Requirement: Shared artifact body is the single source of truth

The system SHALL expose the canonical artifact bodies as `pub const &str` constants from `repograph_core::agent_artifact`: a **consumer** body (read-only surface) and a **setup** body (mutating surface), selected by a `Capability` value. Each constant SHALL contain all repograph-specific instructional prose for its capability and SHALL NOT contain per-agent frontmatter, headers, or wrappers. Per-agent writers SHALL wrap these constants; they SHALL NOT author body content independently.

The consumer body SHALL have two variants, selected by whether the target agent hosts MCP:

- The **policy** variant, used for agents that host MCP (`claude-code`, `cursor`, `windsurf`, `copilot`). It SHALL carry only guidance that the MCP tool schemas cannot express: preferring repograph over generic file-search tools for cross-repo questions, not using it for the current directory's own git status, and not iterating per repo when one call covers the whole registry. It SHALL NOT contain a command-surface table or a JSON-envelope reference, because the advertised tool schemas carry both.
- The **CLI** variant, used for agents that host no MCP (`aider`, `agents-md`). It SHALL retain the purpose statement, when-to-invoke triggers, command surface table, and JSON schema cross-reference, since for these agents the CLI is the only surface.

Both consumer variants SHALL reference only the read-only surface. Mutating commands (`add`, `remove`, `edit`, `workspace`, `init`) SHALL NOT appear in either consumer variant's command references. Both SHALL include negative guidance that the agent must not invoke mutating registry commands on its own initiative, and SHALL delegate those operations to the setup skill (`repograph-setup`) rather than dead-ending at "ask the user."

The **setup** body's "Commands" section SHALL cover the mutating surface (`add`, `remove`, `edit`, `workspace create/add/remove/rm`) and SHALL instruct a plan → confirm → execute → verify workflow: resolve and validate inputs, present the concrete plan to the user, mutate only on confirmation, then verify via the command's `--json` confirmation envelope.

#### Scenario: Both bodies are exported once

- **WHEN** `repograph_core::agent_artifact` is consumed
- **THEN** a `pub const` consumer body and a `pub const` setup body each exist and are referenced by every per-agent writer for their capability; no writer duplicates body prose

#### Scenario: Every command name in each body is a real subcommand

- **WHEN** a test parses each body for `repograph <subcommand>` tokens and queries `<Cli as clap::CommandFactory>::command()` for each subcommand name
- **THEN** every name resolves to a real `clap` subcommand; no dead references exist

#### Scenario: Mutating commands are excluded from both consumer variants

- **WHEN** each consumer variant is searched for `repograph add`, `repograph remove`, `repograph edit`, `repograph workspace`, `repograph init`
- **THEN** none appear

#### Scenario: Consumer body delegates mutation to the setup skill

- **WHEN** each consumer variant's "Things to avoid" section is searched for the don't-mutate guidance
- **THEN** it contains an explicit reminder not to invoke mutating registry commands automatically AND names the `repograph-setup` skill as the surface that handles registration, grouping, and edits

#### Scenario: Setup body covers the mutating surface with a confirm-before-write workflow

- **WHEN** the setup body's `## Commands` section is inspected
- **THEN** it references `add`, `remove`, `edit`, and `workspace` subcommands, and the body instructs the agent to present a plan and obtain user confirmation before running any mutation and to verify via the `--json` confirmation envelope

#### Scenario: MCP-hosting agents receive the policy variant

- **WHEN** artifacts are rendered for `claude-code`, `cursor`, `windsurf`, and `copilot`
- **THEN** each contains the policy variant, and neither a command-surface table nor a JSON-envelope section appears

#### Scenario: Non-MCP agents receive the CLI variant

- **WHEN** artifacts are rendered for `aider` and `agents-md`
- **THEN** each contains the CLI variant including its command-surface table

#### Scenario: Variant selection is pinned per agent

- **WHEN** a test asserts the agent-to-variant mapping against a pinned expected table
- **THEN** the test fails if an agent's variant changes without updating the table

## ADDED Requirements

### Requirement: Artifact body version increments so installed artifacts refresh

`ARTIFACT_BODY_VERSION` SHALL be incremented as part of this change so that `refresh_installed_artifacts` rewrites managed sections already present on disk. The existing delimiter contract SHALL continue to make rewriting idempotent, and content outside the managed delimiters SHALL be preserved unchanged.

#### Scenario: An artifact installed at the previous version is refreshed

- **WHEN** an artifact containing a managed section stamped at the previous body version is present and a refresh is performed
- **THEN** the managed section is replaced with the current body and the new version stamp

#### Scenario: Unmanaged content is preserved across the refresh

- **WHEN** the file also contains user-authored content outside the managed delimiters
- **THEN** that content is byte-identical after the refresh

#### Scenario: Refreshing twice is a no-op

- **WHEN** a refresh is performed twice in succession
- **THEN** the file is unchanged after the second refresh

### Requirement: The policy variant tells agents what to do when no MCP tools are present

Because an existing install keeps working until `repograph init` is re-run, the policy variant SHALL state that when no repograph MCP tools are available, the equivalent CLI commands are to be used instead. This SHALL be a single fallback sentence, not a restored command table.

#### Scenario: Fallback guidance is present without reintroducing the table

- **WHEN** the policy variant is inspected
- **THEN** it names the CLI as the fallback when repograph MCP tools are unavailable, and still contains no command-surface table
