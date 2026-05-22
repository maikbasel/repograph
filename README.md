# repograph

[![CI](https://github.com/maikbasel/repograph/actions/workflows/ci.yml/badge.svg)](https://github.com/maikbasel/repograph/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/repograph.svg)](https://crates.io/crates/repograph)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A CLI tool for registering, grouping, and exposing local git repositories as structured context for AI agents and humans alike.

## Commands

| Command | Purpose |
|---------|---------|
| `repograph add <path>` | Register a local git repository (validated via `git2`). Stores the canonical absolute path. |
| `repograph init` | Interactive setup — pick the agent toolchain(s) you use; bulk-register repos found under your projects root (multiselect) and assign them to a workspace in one pass; optionally add more at custom paths. Re-running shows a settings panel for editing the selection or resetting everything. Non-interactive: `--no-prompt --agents <list>`. |
| `repograph list [--json] [--workspace <name>]` | List the registered repositories. `--workspace` restricts output to repos in the named workspace. Renders a table on a TTY, JSON envelope when piped or `--json` is set. |
| `repograph remove <name>` | Remove a registered repository by name. Workspace memberships are preserved as dangling references — surface them via `workspace show`. |
| `repograph status [<names>…] [--workspace <name>] [--json] [--fetch]` | Report branch, upstream, ahead/behind, and working-tree state across one, many, or all registered repos. Read-only; zero-network unless `--fetch` is set. |
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
$ repograph init --no-prompt --agents claude-code,cursor
```

`--no-prompt` requires `--agents`. The empty string `--agents ""` is valid and writes `selected = []`.

### Agent registry

`repograph init` writes the user's selection of agent toolchains to `[agents].selected`. Each ID maps to a known set of rule-file patterns inside a repository. The agent context command (Phase 4b, upcoming) will inline these files into its output.

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
| 2 | CLI argument error (clap usage); also: `repograph init` in non-TTY without `--no-prompt --agents`, and (in future commands) agents-not-configured in non-TTY |
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

The library is published separately (`cargo add repograph-core`) so future tools — including a planned MCP server — can share the same domain logic without going through the CLI.

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
