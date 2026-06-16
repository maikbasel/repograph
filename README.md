# repograph

[![CI](https://github.com/maikbasel/repograph/actions/workflows/ci.yml/badge.svg)](https://github.com/maikbasel/repograph/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/repograph.svg)](https://crates.io/crates/repograph)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A CLI tool for registering, grouping, and exposing local git repositories as structured context for AI agents and humans.

[Getting Started](#getting-started) • [Install](#install) • [Commands](#commands) • [JSON output](#json-output-shapes) • [Shell integration](#shell-integration) • [Exit codes](#exit-codes)

## Getting Started

Three steps from zero to "my AI agent knows about my repos".

### 1. Install

`cargo install repograph` if you have a Rust toolchain (1.85+). Without one, use Homebrew, the Linux/macOS shell installer, the Windows PowerShell installer, or a prebuilt binary. Copy-paste commands for every method live under [Install](#install).

### 2. Run interactive setup

```bash
repograph init
```

This walks you through:

- **Pick your agent toolchain(s)**: `claude-code`, `agents-md`, `cursor`, `aider`, `windsurf`, `copilot`. Multi-select; detection-preselected.
- **Pick a projects root**: the directory where your git clones live (e.g. `~/code`). Persisted to config; asked once.
- **Bulk-register repos**: every unregistered git repo under the projects root is offered as a checkbox. Add extra paths outside the root in the same pass.
- **Assign repos to workspaces**: group related repos under a name (e.g. `acme`) for filtered listing and context aggregation.

`repograph init` also drops a per-agent instruction file (a "skill" for Claude Code, `AGENTS.md` for agents-md, `.cursor/rules/repograph.mdc` for Cursor, …) so the agent learns when to call `repograph` on its own. The full path matrix is under [Per-agent artifact installation](#per-agent-artifact-installation).

Non-interactive variant for dotfiles / CI: `repograph init --no-prompt --agents claude-code,cursor --scope user`.

### 3. Use it

```bash
repograph list                                  # see what's registered
repograph status                                # branch + working-tree state across all repos
repograph status --workspace acme               # scoped to one workspace
repograph context --workspace acme              # JSON payload of every CLAUDE.md / AGENTS.md / .cursorrules / … in the workspace
eval "$(repograph switch repograph)"            # cd to a registered repo by name (see Shell integration below for the rg-cd wrapper)
repograph doctor                                # read-only health check: missing paths, dangling members, missing agent docs
```

After `init`, run `repograph doctor` as a smoke test. If the agent artifact landed correctly you'll see a row like `AgentDocPresent  ok  <repo> / claude-code` for every repo × agent combination. Anything other than `ok`/`warn` is a problem; see [Exit codes](#exit-codes) and the [Doctor](#doctor) section for the full check catalog.

## Install

Four ways in, by what you already have:

| You have | Use | Gets you |
|---|---|---|
| `brew` (macOS, Linux) | Homebrew | Auto-upgrades with `brew upgrade` |
| Neither Rust nor brew | Shell / PowerShell installer | Prebuilt binary, no toolchain |
| A Rust toolchain (1.85+) | crates.io | Builds from source |
| An air-gapped or scripted setup | Prebuilt tarball | Manual placement + verification |

### Homebrew (macOS, Linux)

```bash
brew install maikbasel/tap/repograph
```

### Shell installer (Linux, macOS)

Fetches the prebuilt binary for your platform into `~/.cargo/bin` (or `$CARGO_HOME/bin`). No Rust toolchain needed.

```bash
curl -LsSf https://github.com/maikbasel/repograph/releases/latest/download/repograph-installer.sh | sh
```

### PowerShell installer (Windows)

```powershell
irm https://github.com/maikbasel/repograph/releases/latest/download/repograph-installer.ps1 | iex
```

### From crates.io

Compiles from source, so you need Rust 1.85 or newer.

```bash
cargo install repograph
```

### Pre-built binaries

Tarballs for `x86_64-linux-gnu`, `aarch64-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, and `x86_64-pc-windows-msvc` hang off every [GitHub Release](https://github.com/maikbasel/repograph/releases), each with a SHA-256 checksum and a detached GPG signature (`.asc`). Verify a download against the maintainer's key before running it; steps are in [`SECURITY.md`](SECURITY.md).

## Updating

How you update depends on how you installed:

| Installed via | Update with |
|---|---|
| Homebrew | `brew upgrade repograph` |
| crates.io | `cargo install repograph` |
| Shell / PowerShell installer, or a tarball | `repograph update` — upgrades the binary in place, verifying the download's checksum |

`repograph update` is install-method aware: on a Homebrew or `cargo install` build it changes nothing and prints the package-manager command above instead of clobbering a managed binary. Use `repograph update --check` to see whether a newer version exists without installing it.

In an interactive terminal, repograph also prints a one-line notice on **stderr** when a newer version is available. The check runs at most once per 24 hours and never touches stdout. Silence it by setting either `REPOGRAPH_NO_UPDATE_CHECK` or the cross-tool `NO_UPDATE_NOTIFIER` environment variable.

## Commands

| Command | Purpose |
|---------|---------|
| `repograph add <path>` | Register a local git repository (validated via `git2`). Stores the canonical absolute path. |
| `repograph completions <shell>` | Emit a static completion script for `bash`, `zsh`, `fish`, `powershell`, or `elvish` on stdout. Generated against the live `Cli` so the script never drifts from the actual command surface. |
| `repograph context [<repos>…] [--workspace <name>] [--json]` | Aggregate per-repo agent docs (`CLAUDE.md`, `AGENTS.md`, `.cursor/rules/*.md`, `.cursorrules`, `CONVENTIONS.md`, `.windsurfrules`, `.github/copilot-instructions.md`) into one payload. JSON when piped or `--json`; Markdown when stdout is a TTY (paste-ready into a chat). Per-repo / per-file failures surface as inline warnings, not aborts. |
| `repograph doctor [--json]` | Read-only health check over the config and every registered repo: missing paths, dangling workspace members, missing agent docs, malformed config, and search-index freshness. Coloured `comfy-table` summary on TTY; `schema_version: 1` JSON envelope when piped or `--json`. Zero-network. |
| `repograph index [--workspace <name>] [--semantic]` | Build or refresh the cross-repo search index over the git-tracked files of registered repos (or one workspace). Git-aware and incremental: only changed files are re-processed, removed files are purged. `--semantic` adds local embeddings (requires a build with the `semantic` feature; otherwise lexical-only with a stderr notice). No stdout payload; a summary and warnings go to stderr. |
| `repograph find "<query>" [--workspace <name>] [--limit <n>] [--semantic] [--json]` | Find code across all registered repos (or one workspace) by meaning or keyword — locate a reference implementation when you're not sure which repo holds it. Hybrid retrieval (BM25 + optional semantic). Ranked `comfy-table` on TTY; stable `{ schema_version, query, semantic_used, degraded, hits: [...] }` JSON envelope when piped or `--json`. Empty results are success (exit 0); a never-built index is exit 3. |
| `repograph init` | Interactive setup: pick the agent toolchain(s) you use; bulk-register repos found under your projects root (multiselect) and assign them to a workspace in one pass; add more at custom paths. Re-running shows a settings panel for editing the selection or resetting everything. Non-interactive: `--no-prompt --agents <list>`. |
| `repograph list [--json] [--workspace <name>]` | List the registered repositories. `--workspace` restricts output to repos in the named workspace. Renders a table on a TTY, JSON envelope when piped or `--json` is set. |
| `repograph remove <name>` | Remove a registered repository by name. Workspace memberships are preserved as dangling references; surface them via `workspace show`. |
| `repograph status [<names>…] [--workspace <name>] [--json] [--fetch]` | Report branch, upstream, ahead/behind, and working-tree state across one, many, or all registered repos. Read-only; zero-network unless `--fetch` is set. |
| `repograph switch <name>` | Emit `cd <path>` for the named registered repo on stdout, shell-eval-safe (single-quoted when the path contains whitespace or shell metacharacters). Pair with the `rg-cd` shell function below. |
| `repograph update [--check]` | Update repograph in place when it was installed via the shell/PowerShell installer or a tarball (checksum-verified). Homebrew / `cargo install` builds are left untouched — it prints the right package-manager command instead. `--check` reports availability without installing. |
| `repograph workspace create <name> [--description <text>]` | Create an empty workspace. Names must match `^[a-z0-9][a-z0-9-]{0,62}$` and may not be `default`/`all`/`none`. |
| `repograph workspace rm <name>` | Delete a workspace. Registered repos are not touched. |
| `repograph workspace ls [--json]` | List the registered workspaces with name, description, and member count. |
| `repograph workspace show <name> [--json]` | Show one workspace's resolved live members and dangling references. |
| `repograph workspace add <workspace> <repo>…` | Attach one or more registered repos to a workspace. Idempotent on duplicates; atomic on missing repos. |
| `repograph workspace remove <workspace> <repo>…` | Detach repos from a workspace. Does not deregister the repos themselves. |

`add` accepts `--name`, `--description`, and `--stack <a,b,c>` (comma-separated tags). When `--name` is omitted, the path's basename is used.

A global `--config-dir <PATH>` flag overrides the default config directory. The resolution precedence is `--config-dir` > `REPOGRAPH_CONFIG_DIR` env var > the platform default (`$XDG_CONFIG_HOME/repograph`, `~/Library/Application Support/repograph`, `%APPDATA%\repograph`).

A parallel global `--data-dir <PATH>` flag overrides where the search index (`index.db`) and the embedding-model cache live, with precedence `--data-dir` > `REPOGRAPH_DATA_DIR` env var > the platform data default (`$XDG_DATA_HOME/repograph`, `~/Library/Application Support/repograph`, `%APPDATA%\repograph`). The index is a derived, disposable artifact — deleting it just means the next `repograph index` rebuilds from scratch.

### Sample config

```toml
# ~/.config/repograph/config.toml

[repo.api]
path = "/home/user/code/api"
description = "Core HTTP API service"
stack = ["rust"]

[repo.web]
path = "/home/user/code/web"
stack = ["typescript"]

[workspace.acme]
description = "Acme rebuild"
members = [
    "api",
    "web",
]

[agents]
selected = [
    "claude-code",
    "agents-md",
]

[settings]
projects_root = "/home/user/code"
```

`[agents]` is written by `repograph init` (or auto-prompted by future agent-consuming commands on first run). Presence of the section signals "init has run"; an empty `selected = []` is a valid configured state (user opted out of agent docs).

`[settings] projects_root` records where the user keeps their git projects, asked once during `repograph init` and reused by every subsequent repo-registration. The env var `REPOGRAPH_PROJECT_ROOT` overrides this at runtime (useful for CI / sandbox testing / dotfile parity). An empty env value falls through to the config value. Change the stored path anytime via `repograph init` → "Change project root".

### First run

Interactive (TTY):

```bash
$ repograph init
```

A cliclack-driven flow:

1. **Agent multiselect**: detection-preselected, deselectable.
2. **Projects root**: pick from detected candidates (only those containing ≥1 git repo are surfaced), enter a custom path, or skip. Persisted to `[settings] projects_root`; asked only once.
3. **Bulk repo registration**: `multiselect` of unregistered git repos under your projects root (no preselection, opt-in per repo), then an optional `Register a repo at a custom path?` loop for anything outside that root. The free-form path input has filesystem autocomplete (Tab) and `~` expansion. Basename conflicts during bulk add prompt once for an alternative name and otherwise log + skip so the rest of the batch proceeds.
4. **Per-repo workspace routing**: when N>0 repos were registered, an outer `Add these N repos to workspaces?` confirm gates the step. On yes, you first walk an optional create-new loop (gated by a `Create new workspaces first?` confirm when workspaces already exist; entered directly otherwise) to seed the target pool. Then, for each registered repo in turn, a `Workspaces for '<repo>'` multiselect lets you pick that specific repo's workspaces: zero, one, or many. Different repos can land in different workspaces; empty picks leave a repo unassigned. The config is saved once at the end. The success log uses singular wording when exactly one repo lands in exactly one workspace; otherwise a multi-line `workspace assignments:` block lists each assigned repo with its chosen workspaces.
5. **Summary**: final agents / repos / workspaces counts plus next-step hints.

Re-running on an existing config shows a settings panel with actions including "Update agent selection", "Change project root", "Register another repo" (re-enters the bulk flow), "Manage workspaces", "Reset everything", "Cancel".

Inside "Manage workspaces", **Create** asks for a name and then offers an immediate `multiselect` to populate the new workspace with any registered repos (default `yes`; skipped when no repos are registered). **Add members** also uses a `multiselect`, filtered to the repos that are NOT already in the chosen workspace, so a single trip through the menu can add many repos at once. Empty submissions are valid no-ops; if every registered repo is already a member (or the registry is empty), a WARN log explains the no-op and the menu returns without writing.

Non-interactive (CI, dotfiles, non-TTY):

```bash
$ repograph init --no-prompt --agents claude-code,cursor --scope user
```

`--no-prompt` requires `--agents`. The empty string `--agents ""` is valid and writes `selected = []`.

#### `--scope <user|project>`

Where to install per-agent artifacts (see "Per-agent artifact installation" below). Defaults to `user` when omitted in an interactive run. Required under `--no-prompt` when any selected agent has a meaningful scope choice (today: `claude-code`, `windsurf`); project-only agents (`agents-md`, `aider`, `cursor`) fall through to the project path regardless of this flag.

#### `--force`

Overwrite existing artifacts even outside the managed delimiter block. Without this flag, repograph rewrites only the delimited region of pre-existing files (preserving user-authored content above and below); with it, the file is replaced fresh. Use this to re-assert the canonical body after local edits drift, or to remove user content that has accumulated above the managed block.

#### Per-agent artifact installation

`repograph init` writes a native instruction file for each selected agent so the agent's runtime picks it up automatically and learns when to invoke `repograph`. The path matrix is fixed by each agent's convention:

| Agent ID      | User-scope path                                       | Project-scope path                     |
|---------------|-------------------------------------------------------|----------------------------------------|
| `claude-code` | `~/.claude/skills/repograph/SKILL.md`                 | `<cwd>/.claude/skills/repograph/SKILL.md` |
| `agents-md`   | (project-only)                                        | `<cwd>/AGENTS.md`                      |
| `cursor`      | (project-only)                                        | `<cwd>/.cursor/rules/repograph.mdc`    |
| `aider`       | (project-only)                                        | `<cwd>/CONVENTIONS.md`                 |
| `windsurf`    | `~/.codeium/windsurf/memories/repograph.md`           | `<cwd>/.windsurfrules`                 |
| `copilot`     | (deferred, no v1 writer)                             | (deferred, no v1 writer)              |

Files that may already contain user-authored prose (`AGENTS.md`, `CONVENTIONS.md`, `.windsurfrules`) are managed by a delimiter pair (`<!-- repograph:begin --> … <!-- repograph:end -->`); only the delimited region is repograph-managed and only it is rewritten on re-runs. Content above and below the delimiters is byte-preserved. Pass `--force` to replace the whole file with the bare delimited block. Per-agent install outcomes (Written / Unchanged / Skipped / Failed) are logged to stderr. A failure for one agent does not abort the others; the agent-selection persistence already succeeded.

Selecting `copilot` is valid but writes no file in v1; the agent's instruction format varies across surfaces (repo-level, editor-level, Copilot Workspace) and no single converged path covers them yet.

### Agent registry

`repograph init` writes the user's selection of agent toolchains to `[agents].selected`. Each ID maps to a known set of rule-file patterns inside a repository. `repograph context` inlines the matching files into its payload (one per selected agent, scoped to the in-scope repos).

| Agent ID      | File patterns                            |
|---------------|------------------------------------------|
| `claude-code` | `CLAUDE.md`                              |
| `agents-md`   | `AGENTS.md`                              |
| `cursor`      | `.cursor/rules/*.md`, `.cursorrules`     |
| `aider`       | `CONVENTIONS.md`                         |
| `windsurf`    | `.windsurfrules`                         |
| `copilot`     | `.github/copilot-instructions.md`        |

### JSON output shapes

`repograph list --json` emits a `repos`-keyed envelope:

```json
{ "repos": [ { "name": "...", "path": "...", "description": "...", "stack": [...] } ] }
```

Empty registry: `{ "repos": [] }`.

`repograph workspace ls --json` emits a `workspaces`-keyed envelope:

```json
{ "workspaces": [ { "name": "acme", "description": "Acme rebuild", "members": ["api", "ui"] } ] }
```

`repograph workspace show <name> --json` emits a single workspace with members resolved against the registry and a `dangling` array of names whose repos have been deregistered:

```json
{
  "name": "acme",
  "description": "Acme rebuild",
  "members": [ { "name": "ui", "path": "/home/user/code/ui", "description": null, "stack": [] } ],
  "dangling": ["api"]
}
```

`dangling` is always present (even when empty), so agent consumers can detect drift without a key-existence check. A dangling member also produces a `WARN` line on stderr.

`repograph find "<query>" --json` emits a stable, `schema_version`-carrying envelope (`schema_version: 2`). `hits` is always an array (empty on no match); each hit names the repo, the repo-relative path, the 1-based start line, a fused relevance score, and a snippet. `semantic_used` reports whether embedding retrieval actually contributed; `degraded` carries the fallback reason (or `null`) so a stdout-only consumer can detect a keyword-only result without reading stderr:

```json
{
  "schema_version": 2,
  "query": "jwt refresh token rotation",
  "semantic_used": true,
  "degraded": null,
  "hits": [
    { "repo": "api", "path": "src/auth/token.rs", "line": 42, "score": 0.0312, "snippet": "pub fn rotate_refresh_token(..) { .. }" }
  ]
}
```

When `--semantic` is requested but the binary was built without the `semantic` feature (or the index has no embeddings), `find` degrades to keyword-only retrieval: `semantic_used` is `false`, `degraded` names the reason, and a `note:` line is printed to stderr — stdout stays pure data. Build with embeddings via `cargo install repograph --features semantic`.

`repograph status --json` emits a `repos`-keyed envelope of richer per-repo status entries. The `error` field is always present (`null` on healthy rows) so consumers can branch on `repo.error != null` without a key-existence check:

```json
{
  "repos": [
    {
      "name": "api",
      "path": "/home/user/code/api",
      "branch": "main",
      "upstream": "origin/main",
      "ahead": 0,
      "behind": 0,
      "dirty": false,
      "staged": 0,
      "unstaged": 0,
      "untracked": 0,
      "state": "clean",
      "error": null
    },
    {
      "name": "ui",
      "path": "/home/user/code/ui",
      "branch": "feat/x",
      "upstream": "origin/feat/x",
      "ahead": 2,
      "behind": 0,
      "dirty": true,
      "staged": 1,
      "unstaged": 1,
      "untracked": 0,
      "state": "dirty",
      "error": null
    },
    {
      "name": "ghost",
      "path": "/home/user/code/ghost",
      "branch": null,
      "upstream": null,
      "ahead": 0,
      "behind": 0,
      "dirty": false,
      "staged": 0,
      "unstaged": 0,
      "untracked": 0,
      "state": "missing",
      "error": "no such file or directory"
    }
  ]
}
```

`state` is one of `clean`, `dirty`, `detached`, `unborn`, `bare`, `missing`. A missing or broken repo in a batch run does not abort the command (exit `0`); the failing row carries a populated `error` field and a `WARN` line lands on stderr. Asking for a single broken repo by name (`repograph status <name>`) exits `3` instead; that's a request, not a batch.

`repograph status --workspace acme --json` restricts the scope to live members of a workspace (dangling members skipped, parity with `list --workspace`).

`repograph status --fetch` is opt-in and runs `git fetch` against each repo's upstream remote before computing ahead/behind. Without it, no network calls happen. A fetch failure on any one repo populates that repo's `error` field and isolates the failure; the rest of the batch still completes.

### Agent context

`repograph context` produces the payload an AI agent consumes. Three scope modes, mutually exclusive at the CLI layer:

```bash
repograph context                          # every registered repo
repograph context --workspace team-alpha   # members of one workspace
repograph context api ui lib               # explicitly named repos
```

On first use without `[agents]` configured, an interactive TTY prompts through the same multiselect as `repograph init`; non-TTY first use exits `2` and names `repograph init`.

Output mode:

- **TTY default**: Markdown to stdout (paste-ready into Claude / Cursor / ChatGPT). One `## <repo>` section per repo, one `### <agent>` subsection, one fenced code block per matched file. Fences fall back to `~~~` if the file content contains a backtick fence (so an inlined `CLAUDE.md` with code samples renders correctly).
- **`--json` / non-TTY**: single-line JSON object on stdout, versioned by `schema_version`. Each repo block carries `name`, canonical `path`, current `branch` (`null` for detached / unborn / bare / missing), one `agent_docs` entry per selected agent (each with sorted `files`), and an inline `warnings` array. Top-level `warnings` carries global issues.

```json
{
  "schema_version": 1,
  "generated_at": "2026-05-24T14:23:11Z",
  "agents": ["claude-code", "cursor"],
  "scope": { "kind": "workspace", "name": "team-alpha" },
  "repos": [
    {
      "name": "api",
      "path": "/home/user/code/api",
      "branch": "main",
      "agent_docs": [
        {
          "agent": "claude-code",
          "files": [ { "path": "CLAUDE.md", "bytes": 1234, "content": "# api\n…" } ]
        },
        {
          "agent": "cursor",
          "files": [
            { "path": ".cursor/rules/style.md", "bytes": 567, "content": "…" },
            { "path": ".cursorrules", "bytes": 89, "content": "…" }
          ]
        }
      ],
      "warnings": []
    }
  ],
  "warnings": []
}
```

Behavior contract:

- **No truncation.** File contents are inlined verbatim. Total bytes is logged on stderr; downstream tooling owns the context-window budget.
- **Per-file errors are inline, not fatal.** Unreadable, non-UTF-8, or missing files become warning entries on the enclosing repo's `warnings` array; the rest of the payload still ships and exit is `0`.
- **Bounded filesystem walk.** Flat patterns (`CLAUDE.md`) are existence-checked; glob patterns (`.cursor/rules/*.md`) are matched against the entries of their known parent directory only, with no recursion into `node_modules` or anywhere else.
- **Stable ordering.** Top-level `repos` is sorted by name. Each `agent_docs` array preserves the order of `[agents].selected`. Each agent's `files` is sorted by path.
- **Exit codes.** `0` success (including success-with-warnings); `2` for clap usage errors or non-TTY without `[agents]`; `3` for unknown workspace / repo name. `5` is not produced.

### Shell integration

`repograph switch <name>` writes exactly `cd <path>` (and nothing else) to stdout. Wrap it in a one-line shell function so a single command teleports between registered repos:

```bash
# bash / zsh: add to ~/.bashrc or ~/.zshrc
rg-cd() { eval "$(repograph switch "$1")"; }
```

```fish
# fish: add to ~/.config/fish/config.fish
function rg-cd
    eval (repograph switch $argv[1])
end
```

Then `rg-cd api` jumps to the registered repo `api`. Unknown names exit `3` with a `did you mean: …` hint on stderr when there's a near-miss.

`switch` does **not** validate that the path still resolves; that's `repograph doctor`'s job, and the user's shell surfaces a missing-dir `cd` error on its own. Run `repograph doctor` when something looks off.

One-time install of completions per shell (regenerate after upgrading `repograph`):

```bash
# bash (per-user)
repograph completions bash > ~/.local/share/bash-completion/completions/repograph
# zsh (assumes the first fpath entry is user-writable)
repograph completions zsh > "${fpath[1]}/_repograph"
# fish
repograph completions fish > ~/.config/fish/completions/repograph.fish
# powershell (per-session; append to $PROFILE for persistence)
repograph completions powershell | Out-String | Invoke-Expression
# elvish (then `use repograph` in rc.elv)
repograph completions elvish > ~/.config/elvish/lib/repograph.elv
```

Completions are generated against the live `Cli` struct, so they match the subcommands and flags the binary exposes.

### Doctor

`repograph doctor` runs a read-only catalog of checks against the on-disk config and every registered repo. Findings are coloured rows in a `comfy-table` when stdout is a TTY; a `schema_version: 1` JSON envelope when piped or `--json`.

| Check                     | What it verifies                                                                  | Severity on fail |
|---------------------------|-----------------------------------------------------------------------------------|------------------|
| `ConfigPresent`           | Config file exists at the resolved config dir.                                    | `error`          |
| `ConfigParse`             | Config file parses as TOML. Only run when `ConfigPresent` passed.                 | `error`          |
| `AgentsConfigured`        | `[agents]` section is present in the config.                                      | `warn`           |
| `ProjectsRootExists`      | `[settings].projects_root`, if set, points at an existing directory.              | `warn`           |
| `RepoPathExists`          | Per repo: the registered path exists on disk.                                     | `error`          |
| `RepoIsGitRepo`           | Per repo: the path opens as a git repository (gated by `RepoPathExists`).         | `error`          |
| `WorkspaceMembersResolve` | Per workspace member: the member name resolves to a registered repo.              | `warn`           |
| `AgentDocPresent`         | Per repo × per selected agent: at least one file matches the pattern set.         | `warn`           |

Every check that passes emits an `ok` finding too, so you can audit *which* checks ran against *which* targets without consulting the catalog separately.

```json
{
  "schema_version": 1,
  "generated_at": "2026-05-24T14:23:11Z",
  "checks": [
    {
      "check": "RepoPathExists",
      "severity": "error",
      "target": "api",
      "message": "path does not exist: /home/user/code/api"
    },
    {
      "check": "AgentDocPresent",
      "severity": "warn",
      "target": "ui / claude-code",
      "message": "no files matched claude-code patterns (CLAUDE.md)"
    }
  ],
  "summary": { "ok": 12, "warn": 1, "error": 1, "total": 14 }
}
```

Stdout is single-line for clean piping into `jq`. The `checks` array is sorted by `(severity DESC, check ASC, target ASC)`: most pressing first, stable order across runs.

Exit codes:

- `0`: every finding is `ok` or `warn` (warnings do not gate; safe to wire into a `precmd` shell hook).
- `1`: at least one `error` finding. Also returned when the config file is missing; the report is still emitted so you can see what failed.
- `4`: the config file exists but cannot be read (permission denied). No report is emitted; the standard error path takes over.

`doctor` is read-only and zero-network: no config writes, no `git fetch`.

### Filtering by workspace

```bash
repograph list --workspace acme --json
```

Restricts the registry listing to live members of `acme`. Dangling members are skipped (see `workspace show` for the audit view). A non-existent workspace name exits `3`.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General failure (malformed config, runtime usage error, corrupt/unreadable search index) |
| 2 | CLI argument error (clap usage); also: `repograph init` in non-TTY without `--no-prompt --agents`; `--no-prompt` with a scope-bearing agent (`claude-code`, `windsurf`) and no `--scope`; agents-not-configured in non-TTY for agent-consuming commands |
| 3 | Resource not found (path is not a git repo, name not registered, or `repograph find` run before any index was built) |
| 4 | Permission denied (cannot read or write the config file) |
| 5 | Conflict (name or path already registered) |

## Workspace layout

```
crates/
├── repograph-core/   # domain library (no clap, no terminal output)
└── repograph/        # CLI binary, depends on repograph-core
```

The library is published separately (`cargo add repograph-core`) so future tools (alternate front-ends, editor plugins, or batch utilities) can share the same domain logic without going through the CLI. Agent integration ships as native per-agent instruction files written by `repograph init` (see "Per-agent artifact installation" above), not as a separate MCP binary.

## Development

```bash
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo test --workspace --all-features
cargo deny check
```

## Release pipeline

Releases are automated. **Do not** manually tag, bump versions, or edit the changelog.

1. Push conventional commits to `master`.
2. [release-plz](https://release-plz.dev) opens or updates a release PR with `CHANGELOG.md` updates and `Cargo.toml` version bumps. `repograph` and `repograph-core` move in lock-step via a shared `version_group`.
3. Merge the release PR → release-plz publishes both crates to crates.io (core first, then bin) and pushes per-package tags (`repograph-v*`, `repograph-core-v*`).
4. The [`dist`](https://github.com/axodotdev/cargo-dist)-generated `release.yml` workflow fires on the `repograph-v*` tag and:
    - Builds binaries for all five target platforms
    - Uploads tarballs, the source tarball, and SHA-256 checksums to the GitHub Release
    - Publishes shell + PowerShell installers
    - Updates the [`maikbasel/homebrew-tap`](https://github.com/maikbasel/homebrew-tap) formula

The `security.yml` workflow runs `rustsec/audit-check` daily at 00:00 UTC and on demand via `workflow_dispatch`. The `sign.yml` workflow fires after `Release` completes and attaches GPG-detached signatures (`.asc`) for every binary, source tarball, and installer script in the GitHub Release. See [`SECURITY.md`](SECURITY.md) for verification instructions.

## License

Licensed under the [MIT License](LICENSE.md).
