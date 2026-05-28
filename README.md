# repograph

[![CI](https://github.com/maikbasel/repograph/actions/workflows/ci.yml/badge.svg)](https://github.com/maikbasel/repograph/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/repograph.svg)](https://crates.io/crates/repograph)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A CLI tool for registering, grouping, and exposing local git repositories as structured context for AI agents and humans alike.

## Commands

| Command | Purpose |
|---------|---------|
| `repograph add <path>` | Register a local git repository (validated via `git2`). Stores the canonical absolute path. |
| `repograph completions <shell>` | Emit a static completion script for `bash`, `zsh`, `fish`, `powershell`, or `elvish` on stdout. Generated against the live `Cli` so the script never drifts from the actual command surface. |
| `repograph context [<repos>…] [--workspace <name>] [--json]` | Aggregate per-repo agent docs (`CLAUDE.md`, `AGENTS.md`, `.cursor/rules/*.md`, `.cursorrules`, `CONVENTIONS.md`, `.windsurfrules`, `.github/copilot-instructions.md`) into one payload. JSON when piped or `--json`; Markdown when stdout is a TTY (paste-ready into a chat). Per-repo / per-file failures surface as inline warnings, not aborts. |
| `repograph doctor [--json]` | Read-only health check over the config and every registered repo: missing paths, dangling workspace members, missing agent docs, malformed config. Coloured `comfy-table` summary on TTY; `schema_version: 1` JSON envelope when piped or `--json`. Zero-network. |
| `repograph init` | Interactive setup — pick the agent toolchain(s) you use; bulk-register repos found under your projects root (multiselect) and assign them to a workspace in one pass; optionally add more at custom paths. Re-running shows a settings panel for editing the selection or resetting everything. Non-interactive: `--no-prompt --agents <list>`. |
| `repograph list [--json] [--workspace <name>]` | List the registered repositories. `--workspace` restricts output to repos in the named workspace. Renders a table on a TTY, JSON envelope when piped or `--json` is set. |
| `repograph remove <name>` | Remove a registered repository by name. Workspace memberships are preserved as dangling references — surface them via `workspace show`. |
| `repograph status [<names>…] [--workspace <name>] [--json] [--fetch]` | Report branch, upstream, ahead/behind, and working-tree state across one, many, or all registered repos. Read-only; zero-network unless `--fetch` is set. |
| `repograph switch <name>` | Emit `cd <path>` for the named registered repo on stdout, shell-eval-safe (single-quoted when the path contains whitespace or shell metacharacters). Pair with the `rg-cd` shell function below. |
| `repograph workspace create <name> [--description <text>]` | Create an empty workspace. Names must match `^[a-z0-9][a-z0-9-]{0,62}$` and may not be `default`/`all`/`none`. |
| `repograph workspace rm <name>` | Delete a workspace. Registered repos are not touched. |
| `repograph workspace ls [--json]` | List the registered workspaces with name, description, and member count. |
| `repograph workspace show <name> [--json]` | Show one workspace's resolved live members and dangling references. |
| `repograph workspace add <workspace> <repo>…` | Attach one or more registered repos to a workspace. Idempotent on duplicates; atomic on missing repos. |
| `repograph workspace remove <workspace> <repo>…` | Detach repos from a workspace. Does not deregister the repos themselves. |

`add` accepts `--name`, `--description`, and `--stack <a,b,c>` (comma-separated tags). When `--name` is omitted, the path's basename is used.

A global `--config-dir <PATH>` flag overrides the default config directory. The resolution precedence is `--config-dir` > `REPOGRAPH_CONFIG_DIR` env var > the platform default (`$XDG_CONFIG_HOME/repograph`, `~/Library/Application Support/repograph`, `%APPDATA%\repograph`).

### Sample config

```toml
# ~/.config/repograph/config.toml

[repo.changelog-x]
path = "/home/maik/IdeaProjects/changelog-x"
description = "Conventional-commits changelog generator"
stack = ["rust"]

[repo.repograph]
path = "/home/maik/IdeaProjects/repograph"
stack = ["rust"]

[workspace.acme]
description = "Acme rebuild"
members = [
    "changelog-x",
    "repograph",
]

[agents]
selected = [
    "claude-code",
    "agents-md",
]

[settings]
projects_root = "/home/maik/IdeaProjects"
```

`[agents]` is written by `repograph init` (or auto-prompted by future agent-consuming commands on first run). Presence of the section signals "init has run"; an empty `selected = []` is a valid configured state (user opted out of agent docs).

`[settings] projects_root` records where the user keeps their git projects, asked once during `repograph init` and reused by every subsequent repo-registration. The env var `REPOGRAPH_PROJECT_ROOT` overrides this at runtime (useful for CI / sandbox testing / dotfile parity). An empty env value falls through to the config value. Change the stored path anytime via `repograph init` → "Change project root".

### First run

Interactive (TTY):

```bash
$ repograph init
```

A cliclack-driven flow:

1. **Agent multiselect** — detection-preselected, deselectable.
2. **Projects root** — pick from detected candidates (only those containing ≥1 git repo are surfaced), enter a custom path, or skip. Persisted to `[settings] projects_root`; asked only once.
3. **Bulk repo registration** — `multiselect` of unregistered git repos under your projects root (no preselection — opt-in per repo), then an optional `Register a repo at a custom path?` loop for anything outside that root. The free-form path input has filesystem autocomplete (Tab) and `~` expansion. Basename conflicts during bulk add prompt once for an alternative name and otherwise log + skip so the rest of the batch proceeds.
4. **Per-repo workspace routing** — when N>0 repos were registered, an outer `Add these N repos to workspaces?` confirm gates the step. On yes, you first walk an optional create-new loop (gated by a `Create new workspaces first?` confirm when workspaces already exist; entered directly otherwise) to seed the target pool. Then, for each registered repo in turn, a `Workspaces for '<repo>'` multiselect lets you pick that specific repo's workspaces — zero, one, or many. Different repos can land in different workspaces; empty picks leave a repo unassigned. The config is saved once at the end. The success log uses singular wording when exactly one repo lands in exactly one workspace; otherwise a multi-line `workspace assignments:` block lists each assigned repo with its chosen workspaces.
5. **Summary** — final agents / repos / workspaces counts plus next-step hints.

Re-running on an existing config shows a settings panel with actions including "Update agent selection", "Change project root", "Register another repo" (re-enters the bulk flow), "Manage workspaces", "Reset everything", "Cancel".

Inside "Manage workspaces", **Create** asks for a name and then offers an immediate `multiselect` to populate the new workspace with any registered repos (default `yes`; skipped when no repos are registered). **Add members** also uses a `multiselect`, filtered to the repos that are NOT already in the chosen workspace — so a single trip through the menu can add many repos at once. Empty submissions are valid no-ops; if every registered repo is already a member (or the registry is empty), a WARN log explains the no-op and the menu returns without writing.

Non-interactive (CI, dotfiles, non-TTY):

```bash
$ repograph init --no-prompt --agents claude-code,cursor --scope user
```

`--no-prompt` requires `--agents`. The empty string `--agents ""` is valid and writes `selected = []`.

#### `--scope <user|project>`

Where to install per-agent artifacts (see "Per-agent artifact installation" below). Defaults to `user` when omitted in an interactive run. Required under `--no-prompt` when any selected agent has a meaningful scope choice (today: `claude-code`, `windsurf`); project-only agents (`agents-md`, `aider`, `cursor`) silently fall through to the project path regardless of this flag.

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
| `copilot`     | (deferred — no v1 writer)                             | (deferred — no v1 writer)              |

Files that may already contain user-authored prose (`AGENTS.md`, `CONVENTIONS.md`, `.windsurfrules`) are managed by a delimiter pair (`<!-- repograph:begin --> … <!-- repograph:end -->`); only the delimited region is repograph-managed and only it is rewritten on re-runs. Content above and below the delimiters is byte-preserved. Pass `--force` to replace the whole file with the bare delimited block. Per-agent install outcomes (Written / Unchanged / Skipped / Failed) are logged to stderr. A failure for one agent does not abort the others — the agent-selection persistence already succeeded.

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
  "members": [ { "name": "ui", "path": "/home/maik/IdeaProjects/ui", "description": null, "stack": [] } ],
  "dangling": ["api"]
}
```

`dangling` is always present (even when empty), making drift trivially detectable by agent consumers. A dangling member also produces a `WARN` line on stderr.

`repograph status --json` emits a `repos`-keyed envelope of richer per-repo status entries. The `error` field is always present (`null` on healthy rows) so consumers can branch on `repo.error != null` without a key-existence check:

```json
{
  "repos": [
    {
      "name": "api",
      "path": "/home/maik/IdeaProjects/api",
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
      "path": "/home/maik/IdeaProjects/ui",
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
      "path": "/home/maik/IdeaProjects/ghost",
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

`state` is one of `clean`, `dirty`, `detached`, `unborn`, `bare`, `missing`. A missing or broken repo in a batch run does not abort the command (exit `0`); the failing row carries a populated `error` field and a `WARN` line lands on stderr. Asking explicitly for a single broken repo (`repograph status <name>`) exits `3` instead — that's a request, not a batch.

`repograph status --workspace acme --json` restricts the scope to live members of a workspace (dangling members silently skipped, parity with `list --workspace`).

`repograph status --fetch` is opt-in and runs `git fetch` against each repo's upstream remote before computing ahead/behind. Without it, no network calls happen. A fetch failure on any one repo populates that repo's `error` field and isolates the failure; the rest of the batch still completes.

### Agent context

`repograph context` is the headline command — it produces the payload an AI agent actually consumes. Three scope modes (mutually exclusive at the CLI layer):

```bash
repograph context                          # every registered repo
repograph context --workspace team-alpha   # members of one workspace
repograph context api ui lib               # explicitly named repos
```

On first use without `[agents]` configured, an interactive TTY prompts through the same multiselect as `repograph init`; non-TTY first use exits `2` and names `repograph init`.

Output mode:

- **TTY default** — Markdown to stdout (paste-ready into Claude / Cursor / ChatGPT). One `## <repo>` section per repo, one `### <agent>` subsection, one fenced code block per matched file. Fences fall back to `~~~` if the file content contains a backtick fence (so an inlined `CLAUDE.md` with code samples renders correctly).
- **`--json` / non-TTY** — single-line JSON object on stdout, versioned by `schema_version`. Each repo block carries `name`, canonical `path`, current `branch` (`null` for detached / unborn / bare / missing), one `agent_docs` entry per selected agent (each with sorted `files`), and an inline `warnings` array. Top-level `warnings` carries global issues.

```json
{
  "schema_version": 1,
  "generated_at": "2026-05-24T14:23:11Z",
  "agents": ["claude-code", "cursor"],
  "scope": { "kind": "workspace", "name": "team-alpha" },
  "repos": [
    {
      "name": "api",
      "path": "/home/maik/IdeaProjects/api",
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
- **Bounded filesystem walk.** Flat patterns (`CLAUDE.md`) are existence-checked; glob patterns (`.cursor/rules/*.md`) are matched against the entries of their known parent directory only — no recursion into `node_modules` or anywhere else.
- **Stable ordering.** Top-level `repos` is sorted by name. Each `agent_docs` array preserves the order of `[agents].selected`. Each agent's `files` is sorted by path.
- **Exit codes.** `0` success (including success-with-warnings); `2` for clap usage errors or non-TTY without `[agents]`; `3` for unknown workspace / repo name. `5` is not produced.

### Shell integration

`repograph switch <name>` writes exactly `cd <path>` (and nothing else) to stdout. Wrap it in a one-line shell function so a single command teleports between registered repos:

```bash
# bash / zsh — add to ~/.bashrc or ~/.zshrc
rg-cd() { eval "$(repograph switch "$1")"; }
```

```fish
# fish — add to ~/.config/fish/config.fish
function rg-cd
    eval (repograph switch $argv[1])
end
```

Then `rg-cd api` jumps to the registered repo `api`. Unknown names exit `3` with a `did you mean: …` hint on stderr when there's a near-miss.

`switch` does **not** validate that the path still resolves — that's `repograph doctor`'s job, and the user's shell surfaces a missing-dir `cd` error directly. Use `repograph doctor` when something looks drifty.

One-time install of completions per shell (regenerate after upgrading `repograph`):

```bash
# bash (per-user)
repograph completions bash > ~/.local/share/bash-completion/completions/repograph
# zsh (assumes the first fpath entry is user-writable)
repograph completions zsh > "${fpath[1]}/_repograph"
# fish
repograph completions fish > ~/.config/fish/completions/repograph.fish
# powershell (per-session — append to $PROFILE for persistence)
repograph completions powershell | Out-String | Invoke-Expression
# elvish (then `use repograph` in rc.elv)
repograph completions elvish > ~/.config/elvish/lib/repograph.elv
```

Completions are generated against the live `Cli` struct, so they always match the subcommands and flags the binary actually exposes.

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

Every check that passes emits an `ok` finding too — you can audit *which* checks ran against *which* targets without consulting the catalog separately.

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

The actual stdout is single-line for clean piping into `jq`. The `checks` array is sorted by `(severity DESC, check ASC, target ASC)` — most pressing first, stable order across runs.

Exit codes:

- `0` — every finding is `ok` or `warn` (warnings do not gate; safe to wire into a `precmd` shell hook).
- `1` — at least one `error` finding. Also returned when the config file is missing — the report is still emitted so you can see what failed.
- `4` — the config file exists but cannot be read (permission denied). No report is emitted; the standard error path takes over.

`doctor` is read-only and zero-network — no config writes, no `git fetch`.

### Filtering by workspace

```bash
repograph list --workspace acme --json
```

Restricts the registry listing to live members of `acme`. Dangling members are silently skipped (see `workspace show` for the audit view). A non-existent workspace name exits `3`.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General failure (malformed config, runtime usage error) |
| 2 | CLI argument error (clap usage); also: `repograph init` in non-TTY without `--no-prompt --agents`; `--no-prompt` with a scope-bearing agent (`claude-code`, `windsurf`) and no `--scope`; agents-not-configured in non-TTY for agent-consuming commands |
| 3 | Resource not found (path is not a git repo, name not registered) |
| 4 | Permission denied (cannot read or write the config file) |
| 5 | Conflict (name or path already registered) |

## Install

### From crates.io

```bash
cargo install repograph
```

### Homebrew (macOS, Linux)

```bash
brew install maikbasel/tap/repograph
```

### Shell installer (Linux, macOS)

```bash
curl -LsSf https://github.com/maikbasel/repograph/releases/latest/download/repograph-installer.sh | sh
```

### PowerShell installer (Windows)

```powershell
irm https://github.com/maikbasel/repograph/releases/latest/download/repograph-installer.ps1 | iex
```

### Pre-built binaries

Tarballs for `x86_64-linux-gnu`, `aarch64-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, and `x86_64-pc-windows-msvc` are attached to every [GitHub Release](https://github.com/maikbasel/repograph/releases).

## Workspace layout

```
crates/
├── repograph-core/   # domain library (no clap, no terminal output)
└── repograph/        # CLI binary, depends on repograph-core
```

The library is published separately (`cargo add repograph-core`) so future tools — alternate front-ends, editor plugins, or batch utilities — can share the same domain logic without going through the CLI. Agent integration ships as native per-agent instruction files written by `repograph init` (see "Per-agent artifact installation" above), not as a separate MCP binary.

## Development

```bash
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo test --workspace --all-features
cargo deny check
```

## Release pipeline

Releases are fully automated. **Do not** manually tag, bump versions, or edit the changelog.

1. Push conventional commits to `master`.
2. [Release Please](https://github.com/googleapis/release-please) opens or updates a release PR (per crate) with `CHANGELOG.md` updates and `Cargo.toml` version bumps.
3. Merge the release PR → Release Please pushes per-crate tags.
4. The `release-please.yml` workflow publishes both crates to crates.io (core first, then bin).
5. The [`dist`](https://github.com/axodotdev/cargo-dist)-generated `release.yml` workflow fires on the bin's tag and:
    - Builds binaries for all five target platforms
    - Uploads tarballs, the source tarball, and SHA-256 checksums to the GitHub Release
    - Publishes shell + PowerShell installers
    - Updates the [`maikbasel/homebrew-tap`](https://github.com/maikbasel/homebrew-tap) formula

The `security.yml` workflow runs `rustsec/audit-check` daily at 00:00 UTC and on demand via `workflow_dispatch`. The `sign.yml` workflow fires after `Release` completes and attaches GPG-detached signatures (`.asc`) for every binary, source tarball, and installer script in the GitHub Release. See [`SECURITY.md`](SECURITY.md) for verification instructions.

## First-time setup

Five GitHub repository secrets must be configured before the first release will fully succeed (Settings → Secrets and variables → Actions):

| Secret | Purpose |
|---|---|
| `RELEASE_PLEASE_TOKEN` | Fine-grained PAT scoped to this repo with `Contents: Read and write` + `Pull requests: Read and write` + `Issues: Read and write`. Required so release-please's release PR triggers downstream workflows on merge — the default `GITHUB_TOKEN` cannot. |
| `CARGO_REGISTRY_TOKEN` | crates.io API token for publishing both crates. Generate at <https://crates.io/settings/tokens>. |
| `HOMEBREW_TAP_TOKEN` | Fine-grained PAT scoped to [`maikbasel/homebrew-tap`](https://github.com/maikbasel/homebrew-tap) with `Contents: Read and write`. Used by `actions/checkout` in `release.yml` to push the regenerated formula. Shared with `changelog-x`'s release pipeline (same token can be reused). |
| `GPG_PRIVATE_KEY` | ASCII-armored private key block (`gpg --armor --export-secret-keys <key-id>`). Used by `sign.yml` to attach detached signatures to each release. |
| `GPG_PASSPHRASE` | Passphrase for the GPG key. Omit if the key has no passphrase. |

Without `RELEASE_PLEASE_TOKEN`, the merge of a release PR will not fire `release.yml` — the chain breaks at the tag step.

## License

Licensed under the [MIT License](LICENSE.md).
