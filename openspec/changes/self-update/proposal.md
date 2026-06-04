## Why

`repograph` ships through four channels — crates.io, a Homebrew tap, the cargo-dist shell/PowerShell installers, and raw prebuilt tarballs — but a user who installed it has no in-tool signal that a newer version exists, and no first-party way to upgrade an installer-placed binary. Homebrew and `cargo install` users have `brew upgrade` / `cargo install`, but the shell-installer and tarball users (the "neither Rust nor brew" path the README explicitly targets) are stranded: nothing tells them an update shipped, and nothing updates the binary in place.

The release machinery is already updater-friendly. cargo-dist (`dist` 0.30.3) builds the installers and writes an install *receipt*; release-plz publishes a GitHub Release per version with SHA-256 checksums; `repograph-core`'s `git_release_enable = false` guarantees `/releases/latest` points at the `repograph` tag, not the asset-less library tag. What's missing is the consumer side: a passive "newer version available" nudge and an explicit `repograph update` command that respects how the binary was installed.

## What Changes

- New `repograph update` subcommand that performs a receipt-aware, in-place upgrade:
  - **Receipt present** (shell/PowerShell installer, tarball): checks GitHub Releases, and if newer, downloads + verifies + replaces the running binary via [`axoupdater`](https://github.com/axodotdev/axoupdater) (cargo-dist's own updater library).
  - **No receipt** (Homebrew, `cargo install`): mutates nothing; prints the correct package-manager command (`brew upgrade repograph` / `cargo install repograph`) and exits `0`. The tool never clobbers a package-manager-managed binary.
  - `--check` flag reports availability without installing.
- New passive **update notifier**: after any other command, when stdout is a TTY and the user has not opted out, a single line on **stderr** announces a newer version and points at `repograph update`. The check hits GitHub Releases at most once per ~24h (on-disk cache) and is fail-silent on any error. Opt out via `REPOGRAPH_NO_UPDATE_CHECK` or the cross-tool `NO_UPDATE_NOTIFIER`.
- Both surfaces resolve "latest" from the **same** source — GitHub Releases for `maikbasel/repograph` — so the nudge and the actual upgrade never disagree.
- `dist-workspace.toml` keeps `install-updater = false`: the embedded subcommand replaces the separate companion-binary updater, giving one discoverable command across all install methods instead of a second binary present only for installer-based installs.
- New `RepographError` variants for update failure modes (network/IO, checksum/signature verification, binary-write permission), mapped through the existing `exit_code()` contract.
- README: a `repograph update` command-table row, an "Updating" subsection covering the per-channel behavior, and the two opt-out env vars.
- Tests cover: `update --check` with no receipt prints package-manager guidance (exit `0`); the notifier is suppressed in non-TTY, with `REPOGRAPH_NO_UPDATE_CHECK`, and with `NO_UPDATE_NOTIFIER`; cache TTL expiry and the semver-compare decision are unit-tested without network; live self-update is gated behind an opt-in `#[ignore]` so CI stays hermetic and zero-network.

## Capabilities

### New Capabilities

- `self-update`: defines the `repograph update` subcommand surface (receipt-aware in-place upgrade, `--check` report-only mode, package-manager deferral when no receipt is present), the passive update-notifier (post-command, stdout-TTY-gated, stderr-only single line, ~24h on-disk cache, dual opt-out env vars, fail-silent contract), the shared GitHub-Releases version source (`maikbasel/repograph`, latest-release-is-the-binary-tag invariant), and the exit-code mapping for update failures.

### Modified Capabilities

_None._ `self-update` adds a new subcommand and a post-command hook in `main()`; it does not change the contract of any existing command. `registry-core`, `context-command`, `doctor-command`, etc. are untouched.

## Impact

- **Code**:
  - `crates/repograph/src/commands/update.rs` — new command (`Args`, `run`): receipt detection, `--check`, install vs. defer-to-package-manager.
  - `crates/repograph/src/commands/mod.rs` — register the subcommand.
  - `crates/repograph/src/main.rs` — wire `Update` into the clap dispatch; invoke the notifier at end of `main()`.
  - `crates/repograph/src/update_notify.rs` — new module: gating decision (pure), ~24h disk cache (read/write/expiry), GitHub-Releases version query, single-line stderr render. Confines any async runtime to this and the update module so the rest of the CLI stays sync.
  - `crates/repograph-core/src/error.rs` — new `RepographError` variants for update network/IO, verification, and write-permission failures, with `exit_code()` coverage.
  - `crates/repograph/Cargo.toml` — add `axoupdater` (GitHub-releases + rustls TLS features); whatever async runtime axoupdater requires, confined to the update path.
- **Dependencies**: `axoupdater` (new — first network/HTTP dependency in the tree; pulls a TLS stack — pin to **rustls** so the five cross-compiled targets, including `x86_64-pc-windows-msvc`, need no system OpenSSL). Recorded in `deny.toml`/`Cargo.lock`; Renovate-managed like the rest.
- **Public surface**: one new subcommand (`repograph update [--check]`) and one new passive stderr line. No change to any existing command's stdout contract.
- **Build/release**: `dist-workspace.toml` `install-updater` stays `false` (no behavior change); no change to release-plz, CI, or the homebrew-tap formula.
- **Performance**: the notifier adds one cached GitHub call at most once per ~24h, run after the command's real work, capped by a short timeout, fully fail-silent — zero added latency on a cache hit, bounded on a miss.
- **Docs**: README command table + "Updating" subsection + opt-out env vars; the existing exit-code table is reused (no new codes).
- **Not affected**: `Cargo.lock` version field (release-plz owns it), `CHANGELOG.md`, the existing commands, the JSON payload contracts.
