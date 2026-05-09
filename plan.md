# repograph — Implementation Plan

## Phase 1: Project Skeleton
- Cargo workspace with single `repograph` binary crate
- Full `clap` command tree stubbed (all subcommands, no logic)
- `error.rs` with `RepographError` via `thiserror`
- `output.rs` with `OutputMode` enum and TTY detection via `is-terminal`
- `CLAUDE.md`, `rust-toolchain.toml`, `.gitignore`
- `cargo check` passes with zero warnings

## Phase 2: Core Registry
OpenSpec change: `registry-core`

- `config.rs` — `Config`, `Repo` structs, TOML persistence to `~/.config/repograph/config.toml`
- `repograph add <path> [--name] [--desc] [--stack]` — validates git repo via `git2`, stores metadata
- `repograph list [--json]` — TTY: `comfy-table`, non-TTY: JSON
- `repograph remove <name>` — exits with code 3 if not found

## Phase 3: Workspaces
OpenSpec change: `workspace-support`

- `Workspace` struct added to config model
- `repograph workspace add <name> --repos <r1,r2>` — validates all repos exist
- `repograph workspace list [--json]`
- `repograph workspace set-default <name>`

## Phase 4: Git Introspection
OpenSpec change: `git-status`

- `git.rs` — branch, dirty flag, ahead/behind counts, last commit subject via `git2`
- `repograph status [--workspace] [--json]` — `MultiProgress` spinner per repo, replaced by ✓/✗

## Phase 5: Agent Context
OpenSpec change: `context-command`

- `repograph context [--workspace] [--json]` — combines repo metadata + git state + inlined agent config files (`CLAUDE.md`, `AGENTS.md`, `SOUL.md`, `.cursor/rules`)
- `--fields` flag to limit output size
- `CONTEXT.md` written to `~/.config/repograph/` on first run

## Phase 6: Shell Integration & Polish
OpenSpec change: `shell-integration`

- `repograph switch <name>` — outputs `cd <path>` to stdout only
- `repograph shell-init <fish|bash|zsh>` — prints shell integration snippet
- `repograph doctor` — validates all registered paths, reports stale entries
- `--dry-run` on destructive commands (`remove`, `workspace add`)

## Phase 7: Distribution Setup
- `cargo dist init` — generates `release.yml`, configures Homebrew tap (`maikbasel/homebrew-tap`), shell + PowerShell installers
- `release-please-config.json` + `.release-please-manifest.json`
- `dist-workspace.toml`, `deny.toml`, `renovate.json`
- `HOMEBREW_TAP_DEPLOY_KEY` secret added to repo
- `cargo dist plan` verified clean
- `README.md` with install instructions for all four channels

---

## Milestones

| Phase | Deliverable | Key Commands |
|---|---|---|
| 1 | Skeleton | `cargo check` clean |
| 2 | Registry | `add`, `list`, `remove` |
| 3 | Workspaces | `workspace add/list` |
| 4 | Git state | `status` |
| 5 | Agent context | `context` |
| 6 | Polish | `switch`, `shell-init`, `doctor` |
| 7 | Distribution | all channels live |
