## ADDED Requirements

### Requirement: `repograph update` performs a receipt-aware in-place upgrade

The CLI SHALL accept a `repograph update` subcommand that resolves how the running binary was installed via the cargo-dist install receipt (through `axoupdater`) and acts accordingly:

- When an install receipt is present (shell/PowerShell installer or prebuilt tarball), the command SHALL query the latest GitHub Release for `maikbasel/repograph` and, if it is newer than the running version, download it, verify it against the published checksum, and replace the running binary in place. When the running version is already the latest, the command SHALL report "already up to date" and make no changes.
- When no install receipt is present (Homebrew or `cargo install`), the command SHALL NOT modify any file. It SHALL print the correct upgrade command for that install method (`brew upgrade repograph` or `cargo install repograph`) and exit `0`.

The command SHALL NOT write to stdout for machine consumption; all user-facing status SHALL go to stderr, leaving stdout clean.

#### Scenario: Installer-based install upgrades in place when newer exists

- **WHEN** the binary was placed by the shell installer (an install receipt exists), a newer GitHub Release exists, and the user runs `repograph update`
- **THEN** the newer binary is downloaded, verified against its checksum, and installed in place; stderr reports the new version; exit code is `0`

#### Scenario: Installer-based install reports up-to-date

- **WHEN** an install receipt exists and the running version equals the latest GitHub Release, and the user runs `repograph update`
- **THEN** no file is modified, stderr reports that the binary is already up to date, and exit code is `0`

#### Scenario: Homebrew/cargo install defers to the package manager

- **WHEN** no install receipt is present and the user runs `repograph update`
- **THEN** no file is modified; stderr prints the package-manager upgrade command (`brew upgrade repograph` or `cargo install repograph`); exit code is `0`

### Requirement: `repograph update --check` reports availability without installing

The `repograph update` command SHALL accept a `--check` flag that queries the latest GitHub Release, reports whether an update is available (and to which version) on stderr, and performs no installation regardless of install method. `--check` SHALL exit `0` in both the up-to-date and update-available cases.

#### Scenario: `--check` reports an available update without installing

- **WHEN** a newer GitHub Release exists and the user runs `repograph update --check`
- **THEN** stderr names the available version, no binary is modified, and exit code is `0`

#### Scenario: `--check` reports up-to-date

- **WHEN** the running version equals the latest GitHub Release and the user runs `repograph update --check`
- **THEN** stderr reports up-to-date, no binary is modified, and exit code is `0`

### Requirement: The version source is GitHub Releases for `maikbasel/repograph`

Both `repograph update` and the passive notifier SHALL resolve "the latest version" from the GitHub Releases of the `maikbasel/repograph` repository, and SHALL treat the repository's latest release as the `repograph` binary release. The HTTP client SHALL use the rustls TLS backend.

#### Scenario: Latest-release lookup targets the binary tag

- **WHEN** any version check runs against `maikbasel/repograph`
- **THEN** it reads the repository's latest release (the `repograph` binary tag — `repograph-core` releases are not published as GitHub Releases) and compares its version against the running `CARGO_PKG_VERSION`

### Requirement: A passive update notifier runs after other commands, gated and fail-silent

After a dispatched command returns, the CLI SHALL evaluate whether to emit an update notice. The notice SHALL be emitted only when ALL of the following hold:

1. stdout is a TTY;
2. the dispatched command was not `update`;
3. neither the `REPOGRAPH_NO_UPDATE_CHECK` nor the `NO_UPDATE_NOTIFIER` environment variable is set to a non-empty value.

When emitted, the notice SHALL be a single line written to **stderr** (never stdout) naming the available version, the running version, and how to upgrade. The underlying version check SHALL be throttled by an on-disk cache with a TTL of approximately 24 hours: a check SHALL contact the network only when the cache is absent, malformed, or older than the TTL, and SHALL otherwise reuse the cached result. Any error in the notifier path (network, timeout, IO, parse, cache) SHALL result in no output and SHALL NOT change the exit code produced by the dispatched command.

#### Scenario: Newer version available in an interactive session prints one stderr line

- **WHEN** stdout is a TTY, no opt-out env var is set, a newer version is available (per cache or a fresh check), and the user runs any command other than `update`
- **THEN** exactly one line is written to stderr naming the new and current versions and pointing at `repograph update`; stdout is unaffected; the exit code is the command's own

#### Scenario: Non-TTY stdout suppresses the notifier

- **WHEN** stdout is not a TTY (piped, redirected, or run under automation) and a newer version is available
- **THEN** no update notice is written to stdout or stderr

#### Scenario: `REPOGRAPH_NO_UPDATE_CHECK` suppresses the notifier

- **WHEN** `REPOGRAPH_NO_UPDATE_CHECK=1` is set, stdout is a TTY, and a newer version is available
- **THEN** no update notice is written and no network check is performed

#### Scenario: `NO_UPDATE_NOTIFIER` suppresses the notifier

- **WHEN** `NO_UPDATE_NOTIFIER=1` is set, stdout is a TTY, and a newer version is available
- **THEN** no update notice is written and no network check is performed

#### Scenario: The `update` command does not also trigger the notifier

- **WHEN** the user runs `repograph update` (or `repograph update --check`) in a TTY with a newer version available
- **THEN** the passive notifier does not additionally print; only the command's own output is produced

#### Scenario: Fresh cache avoids a network call

- **WHEN** the on-disk update-check cache is newer than the TTL and the user runs a command in a TTY
- **THEN** no network request is made; any notice is derived from the cached latest version

#### Scenario: Notifier errors are silent and do not affect exit code

- **WHEN** the version check fails (network unreachable, timeout, or GitHub returns an error) during a successful command run in a TTY
- **THEN** no notice and no error are printed by the notifier, and the process exits with the dispatched command's own exit code

### Requirement: Update failures map to the documented exit-code contract

The `repograph update` command SHALL map failures to the existing exit-code contract without introducing new codes: a successful update, an already-current result, a successful `--check`, and the no-receipt package-manager-guidance case SHALL exit `0`; a network or IO failure reaching or downloading the release SHALL exit `1`; a checksum or signature verification failure SHALL exit `1`; an inability to write the replacement binary due to insufficient permissions SHALL exit `4`. Each non-zero outcome SHALL be backed by a `RepographError` variant routed through the existing `exit_code()` mapping.

#### Scenario: Network failure during update exits 1

- **WHEN** an install receipt is present but the latest release cannot be fetched or downloaded due to a network error, and the user runs `repograph update`
- **THEN** stderr names the failure and the exit code is `1`

#### Scenario: Checksum verification failure exits 1

- **WHEN** a release is downloaded but fails checksum verification, and the user runs `repograph update`
- **THEN** the binary is not replaced, stderr names the verification failure, and the exit code is `1`

#### Scenario: Permission-denied on binary write exits 4

- **WHEN** an update is available and verified but the running binary cannot be replaced because the location is not writable, and the user runs `repograph update`
- **THEN** the existing binary is left intact, stderr names the permission failure, and the exit code is `4`

### Requirement: README documents the update command and notifier

The project `README.md` SHALL document the self-update surface, including:

- A `repograph update` row in the command table with a one-line description.
- An "Updating" subsection covering the per-channel behavior (Homebrew/cargo defer to the package manager; installer/tarball update in place) and the `--check` flag.
- The two opt-out environment variables for the passive notifier (`REPOGRAPH_NO_UPDATE_CHECK`, `NO_UPDATE_NOTIFIER`).

#### Scenario: README documents updating and opt-out

- **WHEN** a reader searches `README.md` for `repograph update`
- **THEN** they find the command-table row, the "Updating" subsection describing per-channel behavior and `--check`, and both opt-out environment variables
