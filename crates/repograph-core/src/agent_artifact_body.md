## What repograph is

repograph is a CLI tool that maintains a registry of local git repositories and exposes their state (branches, status, agent-doc content like CLAUDE.md and AGENTS.md, workspace groupings) as structured JSON for AI agents. It runs locally; no network. The user has registered which repos matter to them via `repograph add` and selected their agent toolchains via `repograph init`. Reach for repograph whenever a request would benefit from cross-repo awareness instead of asking the user to paste paths or context manually.

## When to invoke

- The user asks about cross-repo context: "what's in flight across my projects", "show me the state of every repo I have registered".
- The user references multiple projects in one turn: "compare X and Y", "the auth changes in repo A affect repo B".
- The user asks "what repos are registered", "list my repos", or "show me my workspaces".
- The user asks to switch to a repo: "cd into the api repo", "open the cli repo for me", "switch to <name>".
- The user asks for status across projects: "which repos have uncommitted changes", "what's dirty right now".
- The user wants the agent-doc content for one or more repos pulled into the conversation: "load the CLAUDE.md for repo X", "give me the AGENTS.md for workspace acme".
- The user reports something feels off with their setup ("my agent isn't seeing X"): run `repograph doctor --json` to surface health-check findings before guessing.

## Commands

| Intent                                | Command                       |
|---------------------------------------|-------------------------------|
| List registered repos and workspaces  | `repograph list --json`       |
| Show per-repo git status              | `repograph status --json`     |
| Build full agent context for repos    | `repograph context --json`    |
| Resolve a repo to a `cd` target       | `repograph switch <name>`     |
| Diagnose registry health              | `repograph doctor --json`     |

The `--json` form is the agent-facing surface; always pass it. Every command has a TTY-friendly table form for humans, but agents should consume JSON. `repograph switch <name>` prints exactly `cd <quoted-path>` on stdout; use it to ground filesystem operations to a known repo without rebuilding the path yourself.

## JSON envelope

Every JSON response carries a top-level `schema_version` integer field. Today's schema is `1`. The wrapper shape is `{ "schema_version": 1, ... }` plus command-specific fields. Schemas are stable: additive changes (new optional fields) keep `schema_version = 1`; breaking changes bump the version. Read the version field if you're being defensive.

Salient payload shapes:

- `repograph list --json` → `repos`: per-repo records with `name`, `path`, `workspaces`.
- `repograph status --json` → `repos`: per-repo records with `name`, `path`, `branch`, and a `status` summary.
- `repograph context --json` → `repos`: each entry has `name`, `path`, `branch`, `head_short`, `status_summary`, and (when present) `agent_docs` with inlined CLAUDE.md / AGENTS.md content. This is the primary surface for loading context into your reasoning.
- `repograph doctor --json` → `schema_version`, `generated_at`, `checks` (array of findings with severity), and a `summary` block.

## Things to avoid

- Do not run mutating commands automatically. If the user appears to want to register a new repo, ask them to run `repograph add <path>` themselves rather than calling it. The same applies to `repograph remove` and `repograph workspace ...`. The registry is the user's to manage.
- Do not assume any specific repo is registered without checking. Prefer `repograph list --json` (or `repograph context --json` for the full surface) over hardcoded names.
- Do not paste the full `agent_docs` payload back to the user verbatim; it's intended as input to your own reasoning. Summarize when you reply.
- Do not call repograph in a loop. One `repograph context --json` returns every registered repo's context in one envelope; iterating per repo is slower and wasteful.
