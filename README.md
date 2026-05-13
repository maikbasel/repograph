# repograph

[![CI](https://github.com/maikbasel/repograph/actions/workflows/ci.yml/badge.svg)](https://github.com/maikbasel/repograph/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/repograph.svg)](https://crates.io/crates/repograph)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A CLI tool for registering, grouping, and exposing local git repositories as structured context for AI agents and humans alike.

## Commands

| Command | Purpose |
|---------|---------|
| `repograph add <path>` | Register a local git repository (validated via `git2`). Stores the canonical absolute path. |
| `repograph list [--json] [--workspace <name>]` | List the registered repositories. `--workspace` restricts output to repos in the named workspace. Renders a table on a TTY, JSON envelope when piped or `--json` is set. |
| `repograph remove <name>` | Remove a registered repository by name. Workspace memberships are preserved as dangling references — surface them via `workspace show`. |
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
```

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
| 2 | CLI argument error (clap usage) |
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
