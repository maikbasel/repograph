## Why

Agents do not invoke repograph on their own. In observed sessions they reach for `grep` / `find` to locate a repo or prior art, even with the `repograph` skill installed and correctly worded. The cause is structural, not editorial: `Grep` sits in the model's tool list and costs one step, while repograph sits behind a `Skill` retrieval step whose payoff is unknown until after it is paid. Grep wins that comparison every time.

Every mechanism repograph ships today is passive. An MCP server is the one mechanism that puts repograph's capabilities *into the tool list itself*, where selecting the tool **is** the action — letting repograph compete with `Grep` on equal footing instead of from behind a retrieval step.

## What Changes

- **New `repograph mcp serve` subcommand** on the existing binary — stdio transport, JSON-RPC over stdin/stdout, dispatching into `repograph-core`. No second binary, no new cargo-dist target, no new distribution channel.
- **Exactly six read-only MCP tools**: `list`, `status`, `context`, `switch`, `find`, `doctor`. Each carries `readOnlyHint: true` so clients can auto-approve it — a tool that prompts on every call gets avoided by agents and humans alike.
- **Mutating commands are deliberately not exposed.** `add`, `edit`, `remove`, and `workspace …` stay behind the `repograph-setup` skill's plan→confirm→execute flow. The registry remains the user's to manage, and the schema surface stays small.
- **Schema budget is a hard constraint.** Six tools with tight descriptions cost roughly 600–1200 tokens per turn. That budget is the price of the tool-list slot and is not to be spent on breadth.
- **`repograph init` gains MCP registration** per selected agent, using each vendor's documented mechanism rather than hand-editing files repograph does not own.
- **`repograph doctor` gains an MCP registration check** — is the server registered for each selected agent, and does it start.
- **Skill bodies are trimmed from instruction manual to preference policy.** `agent_artifact_body.md` today is largely a command table and a JSON-envelope reference; typed tool schemas now carry those mechanics. What survives is what MCP cannot express: prefer repograph over `Grep` for cross-repo questions, do not use it for the current directory's own git status, do not loop.
- **Supersedes D7** of the archived `2026-05-28-agent-skills` change, which cancelled the planned `repograph-mcp` binary. That decision priced a *separate binary plus a second distribution channel*; a subcommand is neither. Its other premise — that clients did not reliably speak MCP — no longer holds. D7 gets an explicit supersession note rather than being silently contradicted.

Not breaking: every existing CLI surface, JSON envelope, and exit code is unchanged. MCP is additive.

## Capabilities

### New Capabilities

- `mcp-server`: the `repograph mcp serve` subcommand — stdio transport, the six-tool read-only surface, tool schemas and annotations, error mapping from `RepographError` to MCP error responses, and the stdout/stderr discipline the protocol requires.

### Modified Capabilities

- `init-command`: init additionally registers the MCP server with each selected agent that hosts one, and reports registration outcome per agent.
- `doctor-command`: the check catalog gains MCP registration health.
- `agent-skills`: the shared artifact body is rewritten from command reference to preference policy, and the artifact body version increments so installed artifacts refresh.

## Impact

**Code**

- New `crates/repograph/src/mcp/` — tool definitions and dispatch, in the **binary** crate. MCP is a transport/presentation concern, sibling to `output.rs`; putting it here keeps `repograph-core` synchronous and dependency-light for its separate crates.io consumers.
- New `crates/repograph/src/commands/mcp.rs` — the `serve` subcommand and stdio wiring.
- `crates/repograph/src/commands/init.rs` and `crates/repograph-core/src/agent_artifact.rs` — MCP registration alongside artifact installation.
- `crates/repograph-core/src/doctor.rs` — new check variant.
- `crates/repograph-core/src/agent_artifact_body.md` and `agent_artifact_setup_body.md` — trimmed bodies, `ARTIFACT_BODY_VERSION` bump.

**Dependencies**

- Adds `rmcp` (official Rust MCP SDK) to the binary crate only. **No new async runtime**: `tokio` is already a binary dependency (`rt`, `net`, `time`) hosting the current-thread runtime that `commands/update.rs` blocks on for axoupdater. This change adds the `io-std` feature to that existing dependency for stdio transport. `repograph-core` stays fully synchronous.

**Output contract**

- stdio MCP puts JSON-RPC on stdout. The `mcp serve` subcommand is the one command whose stdout is the protocol stream, so `tracing` output to stderr becomes load-bearing rather than merely conventional.

**Distribution**

- Unchanged. Same five targets, same crates.io publish, same Homebrew formula.

**Docs**

- README gains an MCP section and per-agent registration snippets; the "no MCP binary" note from the `agent-skills` era is replaced.
