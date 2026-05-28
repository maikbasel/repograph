## Why

Today the only way an AI agent learns *that* repograph exists and *when* to call it is for the user to write the prose themselves and paste it into their agent's instruction file. The `[agents]` selection in config tells repograph which toolchains the user uses, but nothing flows in the other direction — repograph never tells the agent how to invoke `repograph context`, `repograph status`, `repograph switch`, `repograph list`, or `repograph doctor`.

The previous roadmap targeted a `repograph-mcp` binary to fill this gap by exposing the same surface as MCP tools. That requires a second binary, a second distribution channel, and an MCP runtime. A simpler path covers ~95% of the value: have `repograph init` write a native instruction artifact for each agent the user already selected. Claude Code reads `SKILL.md`; Cursor reads `.cursor/rules/*.mdc`; Aider reads `CONVENTIONS.md`; Windsurf reads `.windsurfrules`; AGENTS.md is the cross-agent standard increasingly honored by all of them. Each picks up its native artifact automatically with no protocol layer in between.

## What Changes

- **New `--scope <user|project>` flag on `repograph init`** (default `user`). Determines whether artifacts go under `$HOME` or under the current working directory. Some agents are project-scope only by convention (`agents-md`, `aider`); the flag is silently ignored for those.
- **New `--force` flag on `repograph init`** that bypasses the delimiter check and overwrites artifact files outright.
- **`init` writes one native artifact per selected agent** after agent selection completes (interactive flow) or alongside the non-interactive `--no-prompt` path:
  - `claude-code` → `<scope-root>/.claude/skills/repograph/SKILL.md`
  - `agents-md` → `<scope-root>/AGENTS.md` (project-scope only)
  - `cursor` → `<scope-root>/.cursor/rules/repograph.mdc`
  - `aider` → `<scope-root>/CONVENTIONS.md` (project-scope only)
  - `windsurf` → `<scope-root>/.windsurfrules`
  - `copilot` → no artifact written in v1 (deferred — Copilot's instruction format is not consistent across surfaces yet)
- **Shared artifact body** lives as a single `const &str` in `repograph-core` so the prose (when to invoke repograph, JSON schema cross-reference, CLI surface table) is authored once. Per-agent writers wrap it in native-format frontmatter/headers only.
- **Idempotent installation** via a managed delimiter pair (`<!-- repograph:begin -->` / `<!-- repograph:end -->`) for artifacts that may already contain user content (`AGENTS.md`, `CONVENTIONS.md`). Identical body → no-op; differing body → rewrite the delimited section only. `--force` overrides this.
- **Interactive `init` flow** prompts for `--scope` after agent selection. `--no-prompt` requires `--scope` to be explicit when at least one selected agent has a meaningful scope choice.
- **README updated** to document the new flag, the per-agent artifact matrix, and the deprecation of the `repograph-mcp` plan.
- **BREAKING (low-impact)**: `repograph init --no-prompt` invocations that previously succeeded with just `--agents` will now fail with exit code `2` when any selected agent has a meaningful scope choice and `--scope` was not provided. Scripts that pass only Claude Code, agents-md, cursor, etc. need to add `--scope user` (or `--scope project`) to be explicit.

## Capabilities

### New Capabilities

- `agent-skills`: the core machinery for installing per-agent instruction artifacts — the shared body, per-agent format writers (frontmatter / headers / file paths), the managed-section delimiter contract, and idempotency rules. Lives in `repograph-core` so it remains presentation-agnostic.

### Modified Capabilities

- `init-command`: gains the `--scope` flag, the `--force` flag, the artifact-installation step that runs after agent selection in both the interactive and non-interactive paths, and updated exit-code wording for the scope-required-but-missing case under `--no-prompt`.

## Impact

- **Code**: new module `crates/repograph-core/src/agent_artifact.rs` (writers + shared body); modifications to `crates/repograph/src/commands/init.rs` (flag wiring, interactive prompt, post-selection install step); README updates; new acceptance tests `crates/repograph/tests/init.rs` (or extension of existing init tests).
- **APIs**: `repograph-core` exports `install_artifacts(agents: &[AgentId], scope: Scope, root: &Path, force: bool) -> Vec<ArtifactResult>` (or equivalent). No public API changes to existing types.
- **Dependencies**: none. All needed primitives (`fs-err`, `serde`, frontmatter via string concat) already in the workspace.
- **Roadmap**: removes the `repograph-mcp` binary from the implicit "future work" list referenced in `README.md:394`, `crates/repograph-core/src/agents.rs` doc comments, and the design.md files for the `registry-core`, `workspace-support`, `git-status`, `context-command`, and `shell-integration` archives. (Archive contents stay frozen; only forward-pointing references in live files get updated.)
- **User scripts**: any non-interactive `repograph init --no-prompt --agents <list>` invocation needs to grow a `--scope <user|project>` argument; documented in README and in the migration note for the change.
