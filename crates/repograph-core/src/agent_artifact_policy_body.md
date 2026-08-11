## What repograph is

repograph maintains a registry of the user's own local git repositories and exposes their paths, branches, status, and agent docs to you as tools. It runs locally; no network.

Its tools appear in your tool list prefixed `repograph_` — `repograph_list`, `repograph_status`, `repograph_context`, `repograph_switch`, `repograph_find`, `repograph_doctor`. Their schemas describe what each one takes and returns, so this document covers only *when to prefer them*, which the schemas cannot express.

## When to reach for it

- **The user names one of their projects and you need its path.** Use `repograph_switch` instead of searching the filesystem. Guessing a path, or running `find` across the home directory, is slower and frequently wrong.
- **The user asks about work spread across projects** — "what's dirty", "what's in flight", "which repos have uncommitted changes". Use `repograph_status`.
- **The user says they solved something before, somewhere else** — "I did this in another repo", "this is already solved somewhere", "use X as reference" — even when they cannot name the repo. Use `repograph_find`. This is cross-repo precedent search, and it is the case where reaching for a generic file search wastes the most time: the answer is in a repository you are not currently in.
- **You need another project's conventions.** Use `repograph_context` to pull its CLAUDE.md / AGENTS.md content in.
- **The user reports their setup behaving oddly.** Run `repograph_doctor` before guessing.

## When not to

- **The current directory's own git state.** Use ordinary `git`. repograph answers which-repo and across-repos questions; it is not a `git status` wrapper.
- **Searching inside the repository you are already working in.** Your normal file-search tools are the right instrument. `repograph_find` is for when the target repo is unknown.
- **Changing the registry.** Registering, grouping into a workspace, renaming, retagging, and removing all belong to the `repograph-setup` skill, which confirms with the user before writing. Do not invoke those commands on your own initiative — the registry is the user's to manage.

## Two habits worth keeping

- **Do not iterate.** One `repograph_context` call covers every registered repo. Calling it once per repo is slower and burns context for no benefit.
- **Do not assume a repo is registered.** Check with `repograph_list` rather than hardcoding a name.

## If the tools are not there

An install that has not been set up for MCP exposes no `repograph_` tools. In that case run the CLI directly — `repograph list --json`, `repograph status --json`, `repograph context --json`, `repograph switch <name>`, `repograph find "<query>" --json`, `repograph doctor --json` — and tell the user `repograph init` will register the tools properly.
