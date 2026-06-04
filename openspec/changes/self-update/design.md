## Context

`repograph` is distributed through four channels with three different update stories:

- **Homebrew** — `brew upgrade` owns the binary; the tool must never overwrite it.
- **`cargo install`** — the user re-runs `cargo install`; again, not ours to overwrite.
- **Shell / PowerShell installer + raw tarball** — *nothing* manages these. This is the gap: no update signal, no upgrade path.

The release side is already updater-shaped. cargo-dist (`dist` 0.30.3) builds the `shell`, `powershell`, and `homebrew` installers and, for the installer paths, writes an **install receipt** recording how and where the binary was placed. release-plz cuts a GitHub Release per version with SHA-256 checksums and detached GPG signatures (per `SECURITY.md`). Critically, `release-plz.toml` sets `git_release_enable = false` on `repograph-core`, so the repo's `/releases/latest` always resolves to the `repograph` binary tag — the invariant both new surfaces depend on.

**Current state**:

- `dist-workspace.toml` has `install-updater = false`; there is no updater of any kind today.
- The CLI is fully synchronous (no async runtime): `main()` parses with clap, dispatches to `commands::*::run`, maps the `Result` through `RepographError::exit_code()` to an `ExitCode`.
- Output contract is settled and enforced: **stdout = data, stderr = diagnostics**; `OutputMode` / TTY decisions use `is-terminal`. Diagnostics go through `tracing` to stderr; default level is TTY-aware (`warn` on a terminal, `info` when piped).
- Env-var convention is the `REPOGRAPH_` prefix wired through clap (`REPOGRAPH_CONFIG_DIR`, `REPOGRAPH_PROJECT_ROOT`).
- No network dependency exists anywhere in the tree yet — `status --fetch` is the only network feature and it goes through `git2`, not HTTP.

**Constraints (from `CLAUDE.md` and `rules/`)**:

- No `unwrap`/`expect` outside test code; every failure maps to a `RepographError` variant with a documented exit code (`0/1/2/3/4/5`).
- `production-grade.md`: no `todo!()`, no half-wired error paths; both the install path and the defer-to-package-manager path ship and are tested.
- `testing.md`: tests use `tempdir` + real fixtures, no mocks; network must not be required for the default test run.
- `tracing` for diagnostics; stdout reserved for command data (the notifier therefore writes to stderr only).

## Goals / Non-Goals

**Goals**

- Tell an interactive user, unobtrusively, when a newer version exists — without slowing the tool down or ever touching stdout.
- Give installer/tarball users a first-party `repograph update` that upgrades the binary in place with checksum verification.
- Do the right, non-destructive thing for Homebrew and `cargo install` users: tell them their package manager's command, change nothing.
- Keep "what's available" and "what gets installed" pinned to the same source so they can never disagree.

**Non-Goals**

- Silent/automatic background updates (Chrome-style). The notifier never installs; the install is always an explicit `repograph update`.
- Interactive "update now? [y/N]" prompting. Notify-only, matching `gh`/`cargo`/`npm` norms and the tool's quiet-stderr philosophy.
- Flipping `install-updater = true` / shipping a separate `repograph-update` companion binary. One embedded subcommand serves every install method.
- A self-update path that bypasses the package manager for brew/cargo installs. We defer, we don't override.
- Re-implementing signature/checksum verification — axoupdater + the cargo-dist release artifacts already carry it.

## Decisions

### Decision 1: `axoupdater` for the self-updater, not `self_update`

The popular `self_update` crate checks GitHub Releases and replaces the binary, but it is **install-method blind** — it will happily overwrite a Homebrew- or cargo-managed binary, which is exactly the failure mode a four-channel distribution must avoid. [`axoupdater`](https://github.com/axodotdev/axoupdater) is cargo-dist's own updater: it reads the install receipt cargo-dist writes, so it self-updates **only** for installer/tarball installs and can detect the absence of a receipt to defer elsewhere. It also reuses the same installer logic + checksum verification that placed the binary originally. For this project the receipt-awareness is the deciding feature.

### Decision 2: Embedded `repograph update` subcommand, not the cargo-dist companion binary

`install-updater = true` would make cargo-dist ship a separate `repograph-update` binary into the installers. That binary only exists for installer-based installs, isn't discoverable from `repograph --help`, and does nothing for the crates.io path. An embedded subcommand is the industry norm for single-binary CLIs (`rustup self update`, `deno upgrade`, `gh`), is discoverable, and lets us own the UX of the defer-to-package-manager branch. `install-updater` stays `false`.

### Decision 3: GitHub Releases (`maikbasel/repograph`) as the single version source

Both the notifier and `update` resolve "latest" from GitHub Releases for `maikbasel/repograph`. This is the source of truth for the prebuilt binaries and exactly what axoupdater installs from, so the "available" nudge and the actual fetch are guaranteed consistent. The `git_release_enable = false` setting on `repograph-core` keeps `/releases/latest` pointed at the `repograph` tag. crates.io was rejected as the notifier source precisely because it is one step removed from the binary artifacts.

### Decision 4: The notifier reuses axoupdater's version query — one HTTP stack, hand-rolled cache

axoupdater (and therefore a TLS/HTTP stack) is a required dependency once `update` exists. Rather than add a second HTTP crate like `update-informer` just for the notifier, the notifier drives axoupdater's version query with an explicitly-set release source (no receipt needed to *ask* "what's latest"). The one thing axoupdater doesn't provide — a throttling cache — we hand-roll: a small JSON file in the OS cache dir holding the last-checked timestamp and last-seen latest version, with a ~24h TTL. The cache read/write, TTL expiry, and semver comparison are pure functions, unit-tested without network. Trade-off: ~30 lines of cache code in exchange for no second HTTP stack and full test control — in keeping with the project's `lto`/`strip`/`codegen-units = 1` size discipline and `cargo-deny` hygiene.

### Decision 5: rustls TLS backend

axoupdater's HTTP client must use **rustls**, not native-tls/OpenSSL. The five release targets include `x86_64-pc-windows-msvc` and the two `*-unknown-linux-gnu` triples; rustls keeps cross-compilation free of system OpenSSL headers and matches cargo-dist's own portable-binary posture. Selected via the appropriate axoupdater feature; verified in `deny.toml`.

### Decision 6: Notifier gating — stdout-TTY + skip-`update` + dual opt-out, run post-command

The notifier runs from `main()` **after** the dispatched command returns (npm-style: the last thing the user sees), and only when **all** hold:

1. **stdout is a TTY** (`std::io::stdout().is_terminal()`). This single gate is the key one: it means a human is interactively reading output, and it automatically excludes `repograph switch` (eval'd), `repograph completions` (redirected), and any `context`/`status`/`doctor` piped to an agent or file. The check is on *stdout*, not stderr, because stdout-TTY is the precise signal for "human reading results."
2. **The command was not `update` itself** — redundant there.
3. **Neither `REPOGRAPH_NO_UPDATE_CHECK` nor `NO_UPDATE_NOTIFIER` is set** (any non-empty value disables).

When it does run, it writes a single line to **stderr** (never stdout, regardless of edge cases) and is fail-silent: any network/IO/parse/timeout error is swallowed with no output and no non-zero exit. The exit code is always whatever the dispatched command produced — the notifier can never change it.

### Decision 7: Confine the async runtime to the update path

axoupdater is async (tokio). Rather than make `main()` async and color the whole CLI, the update command and the notifier each construct a private current-thread tokio runtime inside their own module and block on the axoupdater future there. The rest of the CLI stays synchronous exactly as today. tokio is pulled with the minimal feature set needed to drive a single current-thread runtime.

### Decision 8: Exit-code mapping for `update`

`update` reuses the existing contract — no new codes:

| Outcome | Exit |
|---|---|
| Updated, already-current, or `--check` reported successfully | `0` |
| No receipt → printed package-manager guidance (not an error) | `0` |
| Network / IO failure reaching or downloading the release | `1` |
| Checksum / signature verification failure | `1` |
| Cannot write the new binary (permission denied) | `4` |

New `RepographError` variants back the `1` and `4` paths; the no-receipt case returns `Ok(())`. All `update` user-facing messages go to **stderr** (the command has no machine-readable stdout payload), keeping stdout clean and consistent with every other command.

### Decision 9: `--check` is report-only and always exits 0

`repograph update --check` queries the source, prints whether an update is available (and to what version), and never installs. It exits `0` in both the up-to-date and update-available cases — it is informational, not a gate. (A distinct "update available" scripting exit code is deliberately deferred; YAGNI until something needs it.)

### Decision 10: Cache location and format

The notifier cache is a single JSON file under the platform cache dir (via `dirs`, mirroring how config dir is resolved), e.g. `~/.cache/repograph/update-check.json`, holding `{ "last_checked": <rfc3339>, "latest_seen": "<semver>" }`. A read failure or malformed file is treated as a cache miss (re-check). Honoring `REPOGRAPH_CONFIG_DIR`-style overrides is unnecessary here — the cache is disposable and never authoritative.

## Risks / Trade-offs

- **Binary size / first HTTP dependency.** axoupdater + tokio + a rustls HTTP client is a meaningful addition to a tree that is otherwise sync + `git2` + rayon. Accepted: it is the cost of a first-party updater, isolated behind one feature, and the only network code in the binary. `cargo-deny` and Renovate keep the new subtree honest.
- **GitHub API rate limits.** Unauthenticated `/releases/latest` is limited per IP (~60/h). The ~24h cache + stdout-TTY gate make real-world call volume negligible; a rate-limit response is just another fail-silent miss for the notifier and a plain `1` for an explicit `update`.
- **Receipt absence is load-bearing.** The brew/cargo "defer" branch is keyed on axoupdater failing to load a receipt. If a future cargo-dist changes receipt semantics, the defer detection must be re-validated — covered by the no-receipt acceptance test.
- **Testing network paths.** A real self-update can't run hermetically in CI. Mitigation: the gating decision, cache TTL, and semver compare are pure and fully unit-tested; the no-receipt guidance path is acceptance-tested; the live install is behind an opt-in `#[ignore]`.

## Migration Plan

Purely additive. No config schema change, no change to any existing command, no release-pipeline change (`install-updater` stays `false`). Existing installs gain the notifier and `repograph update` on their next upgrade through whatever channel they already use. No user action required; opt-out is available immediately via the documented env vars.

## Open Questions

_None._ Notifier UX (notify-only), delivery (embedded subcommand via axoupdater), version source (GitHub Releases), and gating (stdout-TTY + dual opt-out, fail-silent) were settled during brainstorming.

## Resolved deviations

Discovered while implementing against axoupdater 0.10.0; the plan above is otherwise as-built.

- **TLS must be supplied by a direct `reqwest` dependency.** axoupdater pulls reqwest *transitively through* `axoasset` with `default-features = false` and **no** TLS feature, so HTTPS to GitHub would silently fail with axoupdater/axoasset features alone (Decision 5 assumed a controllable axoupdater TLS feature). The fix: declare `reqwest = { default-features = false, features = ["rustls", "webpki-roots", "json"] }` directly in `crates/repograph/Cargo.toml`. Cargo unions that onto the shared reqwest, giving the whole graph rustls + bundled webpki roots (portable across the windows-msvc and linux-gnu cross targets, no system OpenSSL). The reqwest 0.13 feature is `rustls`, not `rustls-tls`.
- **The notifier builds its own runtime; we did not enable axoupdater's `blocking` feature.** axoupdater's `blocking` feature only wraps `run`/`is_update_needed`, **not** `query_new_version` (which the notifier needs for the available-version string). Since a hand-built current-thread runtime was required for the notifier regardless, the command path uses the same `runtime()` helper to drive `is_update_needed`/`run` async — keeping one HTTP/runtime stack and avoiding the `tokio["full"]` that `blocking` would pull. `tokio` is a direct dep with only `["rt", "net", "time"]`.
- **Cache stores a Unix timestamp, not RFC 3339** (Decision 10 said RFC 3339). `update-check.json` is `{ "last_checked_unix": <i64>, "latest_seen": "<semver>" }`. Unix seconds drop the `time` `parsing` feature and keep the freshness check a pure integer comparison (`cache_is_fresh`), which is trivially unit-testable. The cache is disposable and non-authoritative, so the shape carries no external contract.
- **`UpdateOutcome` enum mediates logic and presentation.** `selfupdate::run_update` returns an `UpdateOutcome` (`Updated`/`AlreadyCurrent`/`UpdateAvailable`/`DeferToPackageManager`); `output::render_update_outcome` turns it into stderr text. This keeps axoupdater orchestration out of the command module and makes every user-facing line unit-testable against a writer — consistent with the repo's stdout/stderr boundary and `output.rs`-owns-rendering rule.
