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
- The user says they've solved something before and wants the prior art — "I did this in another repo", "this is already solved somewhere", "use repo X as reference" — even when they can't name the repo. Run `repograph find "<description or symbol>" --json` to locate the reference implementation across every registered repo before re-implementing. This is the cross-repo precedent search; it is distinct from inspecting the current repo (use plain git for that).

## Commands

| Intent                                | Command                       |
|---------------------------------------|-------------------------------|
| List registered repos                  | `repograph list --json`       |
| Show per-repo git status              | `repograph status --json`     |
| Build full agent context for repos    | `repograph context --json`    |
| Resolve a repo to a `cd` target       | `repograph switch <name>`     |
| Find a reference impl across repos    | `repograph find "<query>" --json` |
| Build/refresh the search index        | `repograph index`             |
| Diagnose registry health              | `repograph doctor --json`     |

`repograph find` searches a local index built by `repograph index`; if a search reports no index, ask the user to run `repograph index` (it is a mutating-ish, potentially slow operation, so don't run it unprompted). Each hit carries `repo`, `path`, `line`, `score`, and a `snippet`.

The `--json` form is the agent-facing surface; always pass it. Every command has a TTY-friendly table form for humans, but agents should consume JSON. `repograph switch <name>` prints exactly `cd <quoted-path>` on stdout; use it to ground filesystem operations to a known repo without rebuilding the path yourself.

## JSON envelope

All commands write JSON to stdout and diagnostic logs to stderr, so `repograph <cmd> --json 2>/dev/null` always yields clean, parseable JSON — never merge stderr into it.

The richer commands (`context`, `doctor`) carry a top-level `schema_version` integer field; today's schema is `1`. The lighter commands (`list`, `status`) currently emit a bare `{ "repos": [...] }` with no `schema_version`. Read the field defensively (`.schema_version // null`) rather than assuming it's present. Schemas are stable: additive changes (new optional fields) keep `schema_version = 1`; breaking changes bump it.

Salient payload shapes:

- `repograph list --json` → `{ "repos": [...] }`. Each record: `name`, `path`, `description` (often `null`), `stack` (array, often empty). No `schema_version`.
- `repograph status --json` → `{ "repos": [...] }`. Each record: `name`, `path`, `branch`, `upstream`, `ahead`, `behind`, `dirty`, `staged`, `unstaged`, `untracked`, `state` (`"clean"`/`"dirty"`), `error`. No `schema_version`.
- `repograph context --json` → top-level `schema_version`, `generated_at`, `scope`, `agents`, `warnings`, `repos`. Each repo entry: `name`, `path`, `branch`, `warnings`, and (when present) `agent_docs` — an array of `{ agent, files: [...] }` with inlined CLAUDE.md / AGENTS.md content. This is the primary surface for loading context into your reasoning.
- `repograph doctor --json` → `schema_version`, `generated_at`, `checks` (array of findings with `check`, `severity`, `target`, `message`), and a `summary` block (`ok`, `warn`, `error`, `total`).

## Things to avoid

- Do not run mutating commands automatically. This skill is read-only. When the user wants to register a repo, group repos into a workspace, or update an existing entry (`add`, `remove`, `edit`, `workspace …`), that is the job of the `repograph-setup` skill — defer to it rather than calling those commands here. The registry is the user's to manage.
- Do not assume any specific repo is registered without checking. Prefer `repograph list --json` (or `repograph context --json` for the full surface) over hardcoded names.
- Do not paste the full `agent_docs` payload back to the user verbatim; it's intended as input to your own reasoning. Summarize when you reply.
- Do not call repograph in a loop. One `repograph context --json` returns every registered repo's context in one envelope; iterating per repo is slower and wasteful.
