## 1. Dependencies & error surface

- [ ] 1.1 Add `axoupdater` to `crates/repograph/Cargo.toml` with the GitHub-releases feature and a **rustls** TLS backend (no native-tls/OpenSSL); add the minimal `tokio` features needed to drive a single current-thread runtime. Pin versions; let Renovate manage thereafter.
- [ ] 1.2 Add new `RepographError` variants in `crates/repograph-core/src/error.rs` for the update failure modes: a network/IO failure, a checksum/signature verification failure, and a binary-write permission failure. Map the first two to exit `1` and the permission one to exit `4` in the existing `exit_code()` function; never `unwrap`/`expect`.
- [ ] 1.3 Run `cargo check` and `cargo clippy --workspace -- -D warnings`; verify `cargo deny check` passes with the new dependency subtree (licenses + advisories). Commit.

## 2. Shared version source

- [ ] 2.1 Add a small internal helper (in `update_notify.rs` or a shared `update` module) that configures an `axoupdater` release source pointing at GitHub Releases for `maikbasel/repograph` and returns the latest version, driving the async call on a private current-thread `tokio` runtime confined to this module. Keep the rest of the CLI synchronous.
- [ ] 2.2 Add a pure `is_newer(current: &str, latest: &str) -> bool` semver comparison helper (current = `env!("CARGO_PKG_VERSION")`); unit-test equal, newer, older, and pre-release edge cases.
- [ ] 2.3 Run `cargo test -p repograph`; commit.

## 3. Notifier: cache + gating (pure logic first)

- [ ] 3.1 Define the cache file shape `{ last_checked: <rfc3339>, latest_seen: <semver> }` and resolve its path under the platform cache dir via `dirs` (e.g. `~/.cache/repograph/update-check.json`). Implement read/write helpers using `fs_err`; a missing or malformed file is treated as a cache miss.
- [ ] 3.2 Implement `cache_is_fresh(last_checked, now, ttl) -> bool` (~24h TTL) as a pure function taking the timestamps explicitly so it is testable without the clock; unit-test boundary cases (just-under, exactly-at, just-over TTL).
- [ ] 3.3 Implement the pure gating decision `should_notify(stdout_is_tty, command_is_update, env_optouts) -> bool` capturing the three gates (stdout TTY, not the `update` command, neither `REPOGRAPH_NO_UPDATE_CHECK` nor `NO_UPDATE_NOTIFIER` set to a non-empty value); unit-test the full truth table.
- [ ] 3.4 Run `cargo test -p repograph`; commit the pure logic.

## 4. Notifier: wiring into `main()`

- [ ] 4.1 Create `crates/repograph/src/update_notify.rs` exposing `maybe_notify(command_is_update: bool)` that: evaluates `should_notify`, short-circuits if false; reads the cache, and only on a stale/absent/malformed cache performs one network check (short timeout) and rewrites the cache; if a newer version is known, writes a single line to **stderr** naming new + current versions and pointing at `repograph update`. Every error path (network, timeout, IO, parse) is swallowed — no output, no error, no exit-code change.
- [ ] 4.2 Register the module in `crates/repograph/src/main.rs`. Call `update_notify::maybe_notify(matches!(cli.command, Command::Update(_)))` after the dispatched command's `Result` is computed but before returning the `ExitCode`, passing whether stdout is a TTY via `std::io::stdout().is_terminal()`. Ensure the notifier can never alter the returned exit code.
- [ ] 4.3 Run `cargo check` / `cargo clippy --workspace -- -D warnings`; commit the wiring.

## 5. The `repograph update` command

- [ ] 5.1 Create `crates/repograph/src/commands/update.rs` with an `Args` struct (`--check` flag, documented) and `run(args: &Args) -> Result<(), RepographError>`; add `#[tracing::instrument]`.
- [ ] 5.2 In `run`: construct an `axoupdater` instance for `repograph`, attempt to load the install receipt. **No receipt** → print the package-manager guidance to stderr (`brew upgrade repograph` / `cargo install repograph`) and return `Ok(())`. Detect which guidance to show from whatever install-source signal is available; default to listing both if indeterminate.
- [ ] 5.3 **Receipt present, `--check`** → query latest, report availability/up-to-date on stderr, return `Ok(())`. **Receipt present, no `--check`** → run the axoupdater upgrade (download + verify + replace) on the confined runtime; report the resulting version on stderr.
- [ ] 5.4 Map axoupdater errors to the new `RepographError` variants: network/IO → exit `1`, checksum/verify → exit `1`, write/permission → exit `4`. No `unwrap`/`expect`; all user-facing text to stderr; stdout stays empty.
- [ ] 5.5 Register `Update(commands::update::Args)` in `crates/repograph/src/commands/mod.rs` and wire it into the clap `Command` enum + dispatch in `main.rs`, with a doc comment matching the style of the existing subcommands.
- [ ] 5.6 Run `cargo check` / `cargo clippy --workspace -- -D warnings`; commit the command.

## 6. Acceptance tests (assert_cmd)

- [ ] 6.1 Create `crates/repograph/tests/update.rs` with a `tempdir`-isolated fixture (set `REPOGRAPH_CONFIG_DIR` and a temp cache/`HOME` so tests never touch the real user environment or hit the network on the default path).
- [ ] 6.2 Test: `repograph update --check` with no install receipt present prints package-manager guidance to stderr and exits `0`; stdout is empty.
- [ ] 6.3 Test: `repograph update` with no receipt prints guidance, modifies nothing, exits `0`.
- [ ] 6.4 Test: the passive notifier is suppressed when stdout is not a TTY (default under `assert_cmd`) — run a normal command (e.g. `list`), assert no update line on stderr and a clean exit.
- [ ] 6.5 Test: `REPOGRAPH_NO_UPDATE_CHECK=1` and (separately) `NO_UPDATE_NOTIFIER=1` suppress the notifier with no network access — assert no notice and the command's own exit code.
- [ ] 6.6 Test: `repograph update --help` surfaces the command and the `--check` flag.
- [ ] 6.7 Test (`#[ignore]`, opt-in/live-network): exercise a real installer-receipt upgrade end-to-end against GitHub Releases; documented as manual/opt-in so CI stays hermetic.
- [ ] 6.8 Run `cargo test --workspace`; iterate until green; commit.

## 7. README & docs

- [ ] 7.1 Add a `repograph update [--check]` row to the README command table with a one-line description.
- [ ] 7.2 Add an "Updating" subsection after the install section: Homebrew/cargo defer to the package manager; installer/tarball update in place; what `--check` does.
- [ ] 7.3 Document the two notifier opt-out env vars (`REPOGRAPH_NO_UPDATE_CHECK`, `NO_UPDATE_NOTIFIER`) alongside the existing `REPOGRAPH_*` env-var documentation.
- [ ] 7.4 Note that `update` uses exit codes `0/1/4` from the existing contract (no new codes); confirm the existing exit-code table needs no change.

## 8. Final checks & archive readiness

- [ ] 8.1 `cargo build --release` succeeds; `repograph update --check` runs against a real config (manually confirm the no-receipt guidance path on a `cargo install`-ed binary).
- [ ] 8.2 `cargo test --workspace` green; `cargo clippy --workspace -- -D warnings` clean; `cargo deny check` clean.
- [ ] 8.3 Confirm `dist-workspace.toml` still has `install-updater = false` (unchanged) and the release pipeline is untouched.
- [ ] 8.4 Run `openspec validate self-update --type change --strict` — must be green.
- [ ] 8.5 Update `design.md` with any resolved deviations from this plan.
- [ ] 8.6 Tick every task above; commit; ready for archive.
