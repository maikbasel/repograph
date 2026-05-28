## ADDED Requirements

### Requirement: Switch command resolves a registered repo and emits a shell-eval-safe `cd` line

The CLI SHALL accept a `repograph switch <name>` subcommand that looks up `<name>` in the repo registry and emits exactly one line to stdout: `cd ` followed by the repo's canonical absolute path, terminated by a single `\n`. No other bytes SHALL be written to stdout under any circumstance — successful invocations produce exactly the one line; failing invocations produce zero bytes on stdout.

The emitted path SHALL be wrapped in single quotes when the path contains any character in the set `[ \t\n'"$\\\`*?[\]{}();&|<>!#~]` (i.e. shell metacharacters or whitespace); embedded single quotes SHALL be escaped as `'\''` (POSIX single-quote string escaping). Paths that contain none of these characters SHALL be emitted unquoted.

`switch` SHALL NOT call `git2::Repository::open` on the target path. Path validity is the `doctor` command's concern; `switch` trusts the registry and lets the user's shell surface a `cd` failure if the path is stale.

`switch` SHALL NOT accept `--json`. The stdout shape is the same regardless of TTY status — it is always shell-eval-safe.

#### Scenario: Successful switch emits exactly the cd line on stdout

- **WHEN** the config has a registered repo `api` at canonical path `/home/user/code/api` and the user runs `repograph switch api`
- **THEN** stdout is exactly the bytes `cd /home/user/code/api\n` (29 bytes including the trailing newline); exit code is `0`; stderr may contain `tracing` log lines but no banner or duplicate of the stdout content

#### Scenario: Path with whitespace is single-quoted

- **WHEN** the config has a registered repo `my project` at canonical path `/Users/maik/Code Sandbox/my project` and the user runs `repograph switch "my project"`
- **THEN** stdout is exactly `cd '/Users/maik/Code Sandbox/my project'\n`; exit code is `0`

#### Scenario: Path with embedded single quote is escaped

- **WHEN** a registered repo's canonical path is `/tmp/Mike's Repo` and the user runs `repograph switch <name>`
- **THEN** stdout is exactly `cd '/tmp/Mike'\''s Repo'\n` (the `'\''` sequence terminates the single-quoted string, prepends an escaped single quote, and reopens the single-quoted string)

#### Scenario: Stdout-only contract holds across TTY and non-TTY

- **WHEN** the user runs `repograph switch api 2>/dev/null` (suppressing stderr) under both a TTY and a pipe (stdout redirected to `out.sh`)
- **THEN** in both cases stdout (or `out.sh`) contains exactly the `cd <path>\n` line and nothing else; no banner / log line / progress indicator leaks to stdout in either mode

### Requirement: Switch command exits 3 on unknown name with optional "did you mean" suggestion

When `<name>` does not resolve to a registered repo, `switch` SHALL exit with code `3` (`RepographError::NotFound { kind: "repo", name }`). Stdout SHALL contain zero bytes. Stderr SHALL contain the error message naming the lookup that failed; when at least one registered repo name has Levenshtein distance `≤ 2` from `<name>` AND `distance ≤ floor(len(name) / 2)`, stderr SHALL also include a "did you mean: a, b, c?" line listing up to three closest matches in ascending distance order, then ascending name order as a tiebreaker.

When no candidates pass the threshold, no suggestion line SHALL be emitted (the absence of a suggestion is itself a useful signal — the user knows their typo wasn't a near-miss).

#### Scenario: Unknown name exits 3 with empty stdout

- **WHEN** the config registers `api`, `ui`, `lib` and the user runs `repograph switch nope`
- **THEN** stdout is empty (zero bytes), stderr contains a message naming `nope`, and the exit code is `3`

#### Scenario: Near-miss name produces a suggestion line

- **WHEN** the config registers `api`, `ui`, `lib` and the user runs `repograph switch app` (distance 1 from `api`)
- **THEN** stderr contains a "did you mean: api?" line in addition to the not-found error; exit code is `3`; stdout is empty

#### Scenario: No near-miss produces no suggestion

- **WHEN** the config registers only `api` and the user runs `repograph switch zzz` (distance 3, above threshold)
- **THEN** stderr contains the not-found error but NO "did you mean" line; exit code is `3`; stdout is empty

#### Scenario: Multiple close matches are listed in ascending distance then name order

- **WHEN** the config registers `api`, `app`, `aps` and the user runs `repograph switch apt` (distance 1 from each)
- **THEN** stderr's "did you mean" line lists all three in ascending name order (`api, app, aps`) since their distances tie

### Requirement: Switch command does not load `[agents]` or trigger first-run prompts

`switch` SHALL load the config but SHALL NOT call `ensure_agents_configured`. The presence or absence of `[agents]` is irrelevant to teleporting between registered repos. The command SHALL function identically whether `[agents]` is set, empty, or missing.

#### Scenario: Switch works on a config with no `[agents]` section

- **WHEN** the config has registered repos but no `[agents]` section and the user runs `repograph switch api` in a non-TTY pipe
- **THEN** stdout is the `cd <path>\n` line; exit code is `0`; no interactive prompt fires; no `NeedsInit` error is raised

### Requirement: Completions command generates static scripts for every supported shell

The CLI SHALL accept a `repograph completions <shell>` subcommand that writes a completion script to stdout for the requested shell. The `<shell>` positional arg SHALL be one of the values in `clap_complete::Shell` (`bash`, `zsh`, `fish`, `powershell`, `elvish`) and SHALL be parsed by clap; unknown shell values SHALL be rejected by clap with exit code `2`.

The completion script SHALL be generated by `clap_complete::generate(shell, &mut cli, "repograph", &mut io::stdout())` against the live `Cli` struct obtained via `<Cli as clap::CommandFactory>::command()`. This guarantees the script reflects the current subcommand and flag surface — generated scripts cannot drift from the actual binary.

Stdout SHALL contain only the completion script; no banner, no log line, no trailing extra newline beyond what `clap_complete` emits.

#### Scenario: Fish completions contain the canonical fish completion marker

- **WHEN** the user runs `repograph completions fish`
- **THEN** stdout contains at least one line beginning with `complete -c repograph` (the canonical fish `complete` invocation); exit code is `0`; stderr is empty (modulo tracing logs)

#### Scenario: Bash completions contain a bash-style function definition

- **WHEN** the user runs `repograph completions bash`
- **THEN** stdout contains a line declaring `_repograph()` or `_repograph_main()` (the bash completion function name); exit code is `0`

#### Scenario: Zsh completions contain a zsh-style compdef directive

- **WHEN** the user runs `repograph completions zsh`
- **THEN** stdout contains a line beginning with `#compdef repograph`; exit code is `0`

#### Scenario: PowerShell completions contain a Register-ArgumentCompleter call

- **WHEN** the user runs `repograph completions powershell`
- **THEN** stdout contains a line including `Register-ArgumentCompleter`; exit code is `0`

#### Scenario: Elvish completions contain an edit:completion namespace reference

- **WHEN** the user runs `repograph completions elvish`
- **THEN** stdout contains a line referencing `edit:completion`; exit code is `0`

#### Scenario: Unknown shell exits 2 with empty stdout

- **WHEN** the user runs `repograph completions tcsh` (not a supported `clap_complete::Shell` variant)
- **THEN** clap rejects the invocation with exit code `2`; stdout is empty; stderr contains a usage error naming the supported values

#### Scenario: Generated script reflects the live subcommand set

- **WHEN** the user runs `repograph completions bash` against a binary that exposes the subcommands `add`, `context`, `init`, `list`, `remove`, `status`, `workspace`, `switch`, `completions`, `doctor`
- **THEN** stdout's generated script lists every one of those subcommand names; no subcommand defined in the `Cli` enum is absent; no completion is listed for a subcommand that does not exist

### Requirement: Tracing logs entry, success, and error for both shell-integration commands

`switch` and `completions` SHALL emit `tracing` logs at three points per the project logging rule:

- **Entry (`debug`)**: command name and key args (`name` for `switch`, `shell` for `completions`).
- **Success (`info`)**: resolved repo + path for `switch`; shell value for `completions`.
- **Error (`error`)**: the error itself plus relevant context.

Tracing output SHALL go to stderr (per the project-wide tracing setup) and SHALL NEVER leak to stdout, preserving both commands' stdout-only contracts.

#### Scenario: Successful switch emits a debug entry and info success on stderr

- **WHEN** the user runs `repograph switch api` with `RUST_LOG=repograph=debug`
- **THEN** stderr contains a `DEBUG` line naming `switch` and the input name on entry, and an `INFO` line on success with structured fields for the resolved repo name and path

#### Scenario: Successful completions emits a debug entry and info success on stderr

- **WHEN** the user runs `repograph completions fish` with `RUST_LOG=repograph=debug`
- **THEN** stderr contains a `DEBUG` line on entry naming the shell value, and an `INFO` line on success; stdout contains the completion script with no tracing lines interleaved

### Requirement: README documents the switch and completions surface and the companion shell snippets

The project `README.md` SHALL document the `repograph switch` and `repograph completions` subcommands under its command table and SHALL include a "Shell integration" subsection covering:

- The companion shell function for bash/zsh: `rg-cd() { eval "$(repograph switch "$1")"; }`.
- The companion shell function for fish: `function rg-cd; eval (repograph switch $argv[1]); end`.
- The one-time install commands for each supported shell:
  - bash: `repograph completions bash > /etc/bash_completion.d/repograph` (system-wide) or per-user equivalent
  - zsh: `repograph completions zsh > "${fpath[1]}/_repograph"`
  - fish: `repograph completions fish > ~/.config/fish/completions/repograph.fish`
  - powershell: `repograph completions powershell | Out-String | Invoke-Expression` (per-session) or appended to `$PROFILE`
  - elvish: `repograph completions elvish > ~/.config/elvish/lib/repograph.elv` and `use repograph` in `rc.elv`
- The exit-code mapping for `switch` (`0` success / `3` unknown name) and `completions` (`0` success / `2` unknown shell).

#### Scenario: README contains a shell integration section

- **WHEN** a reader opens `README.md` and searches for `Shell integration`
- **THEN** they find a section with the `rg-cd` snippet for at least bash, zsh, and fish, and the one-line completion install command for each supported shell
