## ADDED Requirements

### Requirement: `repograph mcp serve` runs a stdio MCP server on the existing binary

The system SHALL expose a `mcp` subcommand with a `serve` child that speaks the Model Context Protocol as JSON-RPC over stdin/stdout. The server SHALL be part of the `repograph` binary — no additional binary, cargo-dist target, crates.io package, or Homebrew formula. The server SHALL NOT open a network socket or bind a port.

#### Scenario: Server completes an MCP initialize handshake over stdio

- **WHEN** `repograph mcp serve` is spawned as a child process and a JSON-RPC `initialize` request is written to its stdin
- **THEN** a well-formed JSON-RPC `initialize` result is read from its stdout, declaring server name `repograph` and the crate version

#### Scenario: No second binary is produced

- **WHEN** the workspace is built and `cargo dist plan` is run
- **THEN** the artifact set is unchanged from before this change; `repograph` remains the only binary

#### Scenario: `mcp` without a child prints help rather than serving

- **WHEN** `repograph mcp` is invoked with no subcommand
- **THEN** clap help for the `mcp` subcommand group is printed and the exit code is 2

### Requirement: The tool surface is exactly six read-only tools

The server SHALL advertise exactly six tools: `repograph_list`, `repograph_status`, `repograph_context`, `repograph_switch`, `repograph_find`, and `repograph_doctor`. Every tool SHALL be annotated `readOnlyHint: true`. Mutating operations (`add`, `edit`, `remove`, `workspace …`, `init`, `index`) SHALL NOT be exposed as MCP tools.

#### Scenario: `tools/list` returns the closed set

- **WHEN** a `tools/list` request is issued against a running server
- **THEN** exactly six tools are returned, their names are the six listed above, and each carries `readOnlyHint: true`

#### Scenario: Mutating command names are absent from the tool surface

- **WHEN** the serialised `tools/list` response is searched for `add`, `edit`, `remove`, `workspace`, `init`, or `index` as tool names
- **THEN** none appear

#### Scenario: Adding a tool requires updating the closed set

- **WHEN** a test asserts the advertised tool-name set against a pinned expected list
- **THEN** the test fails if a tool is added or removed without updating the list

### Requirement: The advertised schema stays within a bounded token budget

The serialised `tools/list` payload SHALL remain small enough to be affordable on every conversational turn. The system SHALL enforce an explicit upper bound on the serialised schema size so that growth is a deliberate decision rather than drift.

#### Scenario: Schema size is asserted against a ceiling

- **WHEN** the `tools/list` response is serialised and measured
- **THEN** its size is below the pinned ceiling, and the test names the ceiling so raising it is an explicit edit

### Requirement: Tool results reuse the documented JSON envelopes

Each tool's result payload SHALL be identical to the JSON produced by the corresponding `--json` CLI invocation for the same inputs — same keys, same nesting, and the same `schema_version` where the CLI emits one. The MCP layer SHALL NOT introduce payload shapes of its own. `repograph_switch` SHALL be the sole exception: it SHALL return the resolved absolute path as data rather than the shell-oriented `cd <quoted-path>` string the CLI prints.

#### Scenario: Tool output matches CLI `--json` output

- **WHEN** `repograph_list`, `repograph_status`, `repograph_context`, and `repograph_doctor` are called against a fixture registry, and the same commands are run with `--json` against the same registry
- **THEN** the parsed JSON values are equal for each pair

#### Scenario: `repograph_switch` returns a path, not a shell command

- **WHEN** `repograph_switch` is called with a registered repo name
- **THEN** the result carries the resolved absolute path as a data field and contains no `cd ` prefix or shell quoting

#### Scenario: Unknown repo name is a typed tool error

- **WHEN** `repograph_switch` is called with a name that is not registered
- **THEN** the response is a tool error naming the unknown repo, and the server remains available for subsequent requests

### Requirement: `repograph_context` refuses an unconfigured registry

`repograph_context` SHALL return a tool error when no agent selection is configured, rather than returning a payload with empty `agent_docs`. An empty payload would be indistinguishable from "these repos genuinely have no agent docs", which is a silent wrong answer to the question the caller asked. The error message SHALL name `repograph init`, including its non-interactive flag form, since the interactive repair the CLI offers is unavailable to a server.

#### Scenario: Unconfigured registry produces an actionable error

- **WHEN** `repograph_context` is called against a registry with repos registered but no agent selection
- **THEN** the result is a tool error whose message names `repograph init`

#### Scenario: Configured registry returns the context payload

- **WHEN** an agent selection is configured and `repograph_context` is called
- **THEN** the payload is returned and matches `context --json` for the same registry

### Requirement: `repograph_find` uses lexical retrieval

`repograph_find` SHALL query lexically and SHALL NOT request semantic retrieval. Semantic retrieval is an opt-in build feature absent from distributed binaries, and where present it adds model-loading latency to a call made mid-conversation. The CLI's `--semantic` flag remains the surface for the deliberate case.

#### Scenario: Find results do not report semantic retrieval

- **WHEN** `repograph_find` returns a result envelope
- **THEN** `semantic_used` is `false`

### Requirement: `repograph_find` preserves CLI index-freshness semantics

`repograph_find` SHALL apply the same pre-search staleness refresh the `find` command applies, by calling the same core entry point. It SHALL accept an optional parameter that skips the refresh, matching the CLI's `--no-refresh` behaviour including its treatment of a missing index.

#### Scenario: Search reflects uncommitted edits

- **WHEN** a tracked file in a registered repo is modified without committing, and `repograph_find` is called with a query matching the new content
- **THEN** the hit is returned, matching what the `find` command returns for the same query

#### Scenario: Refresh can be skipped

- **WHEN** `repograph_find` is called with the skip-refresh parameter set
- **THEN** no reindex is performed and the existing index is queried as-is, matching `find --no-refresh`

### Requirement: Stdout carries only protocol frames in serve mode

Under `mcp serve`, stdout SHALL carry JSON-RPC frames exclusively. All diagnostics, warnings, and progress SHALL be written to stderr. Progress indicators and spinners SHALL be suppressed for the duration of the server, including on code paths shared with CLI commands.

#### Scenario: Every stdout byte is protocol

- **WHEN** a `serve` process handles a sequence of tool calls that includes an error path and a registered repo whose path no longer exists
- **THEN** every line written to stdout parses as a JSON-RPC message and no diagnostic text appears on stdout

#### Scenario: Warnings still surface on stderr

- **WHEN** a tool is called against a registry containing a repo whose path is missing
- **THEN** a warning is emitted on stderr and the tool result still returns successfully with the degraded state represented in the payload

### Requirement: Errors map to MCP responses without terminating the server

`RepographError` values raised during tool dispatch SHALL be converted to JSON-RPC tool errors carrying the error's user-facing message and its documented exit code as data. The server process SHALL NOT exit on a tool-level failure.

Config SHALL be read per tool call rather than once at startup, so registry edits take effect without restarting connected clients. A config that cannot be read or parsed SHALL therefore surface as a tool error on first use rather than as a startup failure — a client that spawned the server renders tool errors in the conversation, whereas a startup exit code is invisible to the user.

#### Scenario: A failing tool call leaves the server usable

- **WHEN** a tool call fails with a typed error and a subsequent valid tool call is issued on the same connection
- **THEN** the first returns a tool error, the second succeeds, and the process has not exited

#### Scenario: The tool error carries the documented exit code

- **WHEN** a tool is called with the name of a repo that is not registered
- **THEN** the tool error names the repo and reports exit code `3`, matching the CLI's not-found contract

#### Scenario: An unknown tool name does not end the session

- **WHEN** a `tools/call` names a tool that does not exist, followed by a valid `tools/list`
- **THEN** the first is answered with an error and the second succeeds

#### Scenario: Registry edits are visible without a restart

- **WHEN** a repo is registered after the server has already answered a call, and the same tool is called again on the same connection
- **THEN** the new repo appears in the result
