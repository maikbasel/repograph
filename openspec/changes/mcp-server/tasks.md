> Outside-in TDD per `.claude/rules/testing.md`: write the failing acceptance test first in each group, then the implementation. Run `cargo clippy -- -D warnings` before `cargo test` at each group boundary, and commit per completed group per CLAUDE.md.
>
> Groups 1–2 constitute the skeleton phase this change explicitly defines; group 3 resolves it. No `todo!()` survives past group 3.

## 1. Dependencies and subcommand wiring

- [x] 1.1 Add `rmcp` to `crates/repograph/Cargo.toml` with only the features stdio server transport requires; document the choice in a dependency comment matching the existing `tokio` / `reqwest` comment style
- [x] 1.2 Add the `io-std` feature to the existing binary `tokio` dependency; confirm no other feature is pulled in and that `repograph-core` gains no dependency
- [x] 1.3 Add `mcp` as a clap subcommand group with a `serve` child in `main.rs` and `commands/mod.rs`; assert `repograph mcp` with no child prints help and exits 2
- [x] 1.4 Create `crates/repograph/src/commands/mcp.rs` and `crates/repograph/src/mcp/` (module root only); confirm `cargo check` is clean and `cargo dist plan` produces an unchanged artifact set

## 2. Server lifecycle and stdio transport

- [x] 2.1 Acceptance test: spawn `repograph mcp serve` as a child process, write a JSON-RPC `initialize` over stdin, assert a well-formed result declaring server name `repograph` and the crate version
- [x] 2.2 Implement `serve` on a private current-thread tokio runtime following the `commands/update.rs` pattern; `main()` stays synchronous with no `#[tokio::main]`
- [x] 2.3 Install a serve-mode tracing subscriber that cannot write to stdout, and suppress every `indicatif` progress path reachable from shared code
- [x] 2.4 Acceptance test: drive a sequence of tool calls including an error path and a registered repo whose path no longer exists; assert every stdout line parses as JSON-RPC and that a missing-path warning appears on stderr
- [x] 2.5 ~~Acceptance test: `mcp serve` against a malformed config exits with the documented code~~ — **superseded by deviation R1**: config is read per call, so a malformed config is a tool-level error, not a startup failure. Replaced by the registry-edits-without-restart test

## 3. Tool surface and dispatch

- [x] 3.1 Acceptance test: `tools/list` returns exactly the six pinned tool names, each with `readOnlyHint: true`, and contains no mutating command name
- [x] 3.2 Define the six tool schemas with terse descriptions; the description text is the adoption surface, so state what each tool is *for* rather than what it wraps
- [x] 3.3 Implement dispatch for `repograph_list`, `repograph_status`, `repograph_context`, `repograph_doctor` by calling core APIs directly (`Config::load`, `resolve_workspace`, `DoctorReport::run`) — sibling to `commands/`, not a wrapper around it
- [x] 3.4 Implement `repograph_switch` returning the resolved absolute path as data; assert the result carries no `cd ` prefix and no shell quoting
- [x] 3.5 Implement `repograph_find` calling `refresh_stale` exactly as `find.rs` does, with a skip-refresh parameter mirroring `--no-refresh` including its missing-index behaviour
- [x] 3.6 Acceptance test: modify a tracked file without committing, assert `repograph_find` returns the hit and that skip-refresh queries the index as-is
- [x] 3.7 Map `RepographError` to JSON-RPC tool errors carrying the user-facing message; acceptance test that a failed call is followed by a successful one on the same connection with no process exit

## 4. Envelope parity and schema budget

- [x] 4.1 Move any command envelope currently assembled in binary-side rendering into a shared serialisable type so CLI and MCP derive from one definition
- [x] 4.2 Acceptance test: for `list`, `status`, `context`, `doctor`, assert the MCP tool result parses equal to the corresponding `--json` CLI output against the same fixture registry
- [x] 4.3 Add the schema-budget test: serialise `tools/list`, assert its size is under a named ceiling constant, and document that raising the constant is a deliberate decision
- [x] 4.4 Add the closed-set test that fails if a tool is added or removed without updating the pinned name list

## 5. Init registration

- [x] 5.1 Add the closed agent→registration-mechanism mapping alongside the existing `file_patterns()` mapping style, including the no-MCP arm for `aider` and `agents-md`
- [x] 5.2 Implement registration writing the resolved absolute executable path with args `["mcp", "serve"]`; acceptance test against a `tempdir` HOME asserts the Cursor config entry
- [x] 5.3 ~~Prefer `claude mcp add`, fall back to `.mcp.json`~~ — **superseded by deviation R7**: the vendor CLI ignores the injected `home`/`cwd`, desynchronising `register` from `status` and making the test suite mutate real user state. Claude Code now writes `~/.claude.json` / `.mcp.json` directly like every other agent
- [x] 5.4 Implement idempotent merge semantics: preserve unrelated server entries, converge on re-run without duplicating, and correct a stale binary path; one test per behaviour
- [x] 5.5 Report registration outcome per agent on stderr (registered / skipped / failed with reason); assert a failure for one agent does not abort the others and that stdout stays free of the reporting
- [x] 5.6 Acceptance test: an init selecting only `aider` and `agents-md` creates or modifies no MCP config

## 6. Doctor check

- [x] 6.1 Add the MCP registration check to the doctor catalog, emitting one finding per selected MCP-hosting agent and none for non-MCP agents
- [x] 6.2 Implement the three severities: `ok` when registered and resolvable, `warn` when unregistered (message names the fixing command), `error` when the registered command does not resolve
- [x] 6.3 Assert the finding participates in the `summary` counts and the error exit code, and that `schema_version` and the finding shape are unchanged

## 7. Skill body retargeting

- [x] 7.1 Split the consumer body into the policy variant (MCP-hosting agents) and the CLI variant (`aider`, `agents-md`); add the pinned agent→variant mapping test
- [x] 7.2 Write the policy variant: prefer repograph over generic file search for cross-repo questions, not for the current directory's own git status, do not iterate per repo — plus the single fallback sentence naming the CLI when no MCP tools are present
- [x] 7.3 Assert the policy variant contains no command-surface table and no JSON-envelope section, and that neither consumer variant references a mutating command
- [x] 7.4 Carry forward the existing consumer guarantees to both variants: the real-subcommand parse test and the `repograph-setup` delegation test
- [x] 7.5 Increment `ARTIFACT_BODY_VERSION`; test that a previous-version managed section refreshes, that unmanaged content is byte-identical afterwards, and that a second refresh is a no-op

## 8. Documentation and decision record

- [x] 8.1 Add a README MCP section: what `repograph mcp serve` is, the six tools, and per-agent registration snippets; replace the "agent integration ships as native artifacts, not as a separate MCP binary" claim, which this change makes false
- [x] 8.2 Update the CLAUDE.md architecture paragraph that states agent integration is not via MCP, and add `crates/repograph/src/mcp/` to the SAFE TO MODIFY list
- [x] 8.3 Record the supersession of D7 in this change's `design.md` decision log — the archived `2026-05-28-agent-skills` files stay frozen per convention; the current record carries the correction
- [x] 8.4 Remove or correct the remaining "future MCP server" doc comments in `repograph-core/src/lib.rs` and `doctor.rs`, which now describe something that exists in the binary rather than something planned

## 10. Post-upgrade reconciliation

- [x] 10.1 Add `setup_version` to `Settings`; assert it round-trips through save/load and is omitted when unset
- [x] 10.2 Add a `reconcile` module in the binary: when agents are configured and the stamp differs from the running version, refresh managed artifacts and register MCP for the already-selected agents, then write the stamp
- [x] 10.3 Wire it into the post-command hook alongside `selfupdate::notify`, fully fail-silent — a reconciliation error must never change the triggering command's exit code
- [x] 10.4 `init` writes the current stamp on completion so a fresh install never reconciles on its first command
- [x] 10.5 Acceptance test: a config with a stale stamp gains an MCP registration after any command, and the stamp advances
- [x] 10.6 Acceptance test: reconciliation runs once, not once per command
- [x] 10.7 Acceptance test: a config with no `[agents]` section is left entirely untouched, stamp included
- [x] 10.8 Acceptance test: an unwritable registration target logs to stderr and leaves the triggering command's exit code intact

## 9. Verification

- [x] 9.1 `cargo clippy -- -D warnings` clean across the workspace
- [x] 9.2 `cargo test` green, including the new stdout-purity, envelope-parity, and schema-budget tests
- [ ] 9.3 `cargo dist plan` runs clean with an unchanged artifact set — **BLOCKED**: `cargo-dist` is not installed locally. No new binary or build target was added, so the artifact set should be unchanged, but this is unverified
- [ ] 9.4 Manual validation with a real client — **NEEDS YOU**: the protocol side is proven (real child process, `initialize` + `tools/list` + `tools/call` over a pipe, 18 acceptance tests), but whether an agent *chooses* `repograph_find` unprompted can only be observed in a live session
- [x] 9.5 `openspec validate mcp-server` passes
