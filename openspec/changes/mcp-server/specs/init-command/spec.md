## ADDED Requirements

### Requirement: Init registers the MCP server with each selected MCP-hosting agent

After agent selection and artifact installation, `repograph init` SHALL register the `repograph mcp serve` server with each selected agent that hosts MCP, using that vendor's documented mechanism. The registration mapping SHALL be a fixed closed set:

| Agent | Mechanism |
|---|---|
| `claude-code` | `claude mcp add` when the `claude` CLI is on `PATH`; otherwise write the `.mcp.json` config |
| `cursor` | `~/.cursor/mcp.json` (user scope) or `.cursor/mcp.json` (project scope) |
| `windsurf` | `~/.codeium/windsurf/mcp_config.json` |
| `copilot` | `.vscode/mcp.json` |
| `aider`, `agents-md` | no MCP host — no registration attempted |

The registered command SHALL use the resolved absolute path of the running executable rather than the bare name `repograph`, so that a client spawning the server does not depend on inherited `PATH`.

#### Scenario: Selecting an MCP-hosting agent writes a registration

- **WHEN** `repograph init` completes with `cursor` selected at user scope
- **THEN** `~/.cursor/mcp.json` contains an `mcpServers` entry keyed `repograph` whose command is the absolute path of the running binary with args `["mcp", "serve"]`

#### Scenario: Agents without an MCP host are skipped

- **WHEN** `repograph init` completes with only `aider` and `agents-md` selected
- **THEN** no MCP registration file is created or modified, and the run succeeds

#### Scenario: Claude Code prefers its own CLI

- **WHEN** `repograph init` completes with `claude-code` selected and a `claude` executable present on `PATH`
- **THEN** registration is performed by invoking `claude mcp add` rather than by hand-writing the config

#### Scenario: Claude Code falls back to config when the CLI is absent

- **WHEN** `repograph init` completes with `claude-code` selected and no `claude` executable on `PATH`
- **THEN** the `.mcp.json` config is written directly and a note explaining the fallback is emitted on stderr

### Requirement: MCP registration is idempotent and preserves unrelated entries

Registration SHALL merge a `repograph` key into any existing server map rather than replacing the file. Entries for other servers SHALL be preserved byte-for-byte in content. Re-running registration SHALL converge to the same result without duplicating entries.

#### Scenario: Existing servers survive registration

- **WHEN** a client config already contains an unrelated MCP server entry and `repograph init` registers
- **THEN** the unrelated entry is still present with its original configuration, alongside the new `repograph` entry

#### Scenario: Re-running init does not duplicate the entry

- **WHEN** `repograph init` is run twice with the same agent selection
- **THEN** exactly one `repograph` entry exists in the config after the second run

#### Scenario: A stale binary path is corrected on re-run

- **WHEN** a `repograph` entry exists pointing at a path that no longer resolves, and `repograph init` is re-run
- **THEN** the entry's command is updated to the current executable path

### Requirement: Registration outcome is reported per agent on stderr

`repograph init` SHALL report the MCP registration result for each selected agent — registered, skipped because the agent hosts no MCP, or failed with the reason. A registration failure SHALL NOT abort artifact installation for the remaining agents.

#### Scenario: A failing registration does not abort the run

- **WHEN** registration for one agent fails because its config directory is not writable, and another selected agent registers successfully
- **THEN** the failure is reported on stderr with its reason, the second agent is still registered, and init completes

#### Scenario: Diagnostics stay off stdout

- **WHEN** `repograph init` runs non-interactively and its stdout is captured separately from stderr
- **THEN** all registration reporting appears on stderr and stdout remains free of it

### Requirement: Setup reconciles itself after a version upgrade

`[settings]` SHALL carry a `setup_version` stamp recording the `repograph` version that last completed setup. After any successful command, when an agent selection is configured and the stamp differs from the running binary's version, the system SHALL re-run the idempotent parts of setup for the already-selected agents — refreshing managed artifact sections and registering the MCP server — and then write the current version to the stamp.

Reconciliation SHALL NOT prompt, SHALL NOT add or remove agents, and SHALL NOT alter the configured scope. It SHALL operate only on regions it already manages. Any failure SHALL be logged to stderr and SHALL NOT alter the exit code of the command that triggered it.

A configuration with no agent selection SHALL be left untouched, including its stamp.

#### Scenario: An upgraded binary registers MCP without a manual init

- **WHEN** a config carries a selected agent and a `setup_version` older than the running binary, and any command is run
- **THEN** the MCP server is registered for that agent and the stamp is updated to the running version

#### Scenario: Reconciliation runs once, not once per command

- **WHEN** a command is run twice in a row after the stamp has been brought current by the first run
- **THEN** the second run performs no reconciliation work

#### Scenario: An unconfigured install is left alone

- **WHEN** a config has no `[agents]` section and a command is run
- **THEN** no artifact is written, no MCP registration is created, and no stamp is written

#### Scenario: A reconciliation failure cannot fail the command

- **WHEN** reconciliation cannot write its registration target because the directory is not writable
- **THEN** the diagnostic appears on stderr and the triggering command still reports its own exit code

#### Scenario: Stamp survives a config round-trip

- **WHEN** a config containing `setup_version` is loaded and saved
- **THEN** the stamp is preserved
