## 1. Core: doctor module scaffolding

- [x] 1.1 Create `crates/repograph-core/src/doctor.rs` with a module-level doc comment summarising the read-only check catalog and zero-network contract
- [x] 1.2 Define the `Severity` enum (`Ok`, `Warn`, `Error`) with `serde::Serialize` rendering lowercase variant names (`"ok"`, `"warn"`, `"error"`); derive `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `PartialOrd`, `Ord` with the `Ord` impl ranking `Error > Warn > Ok` (so the JSON sort works without a custom comparator)
- [x] 1.3 Define the `Check` enum (closed set: `ConfigPresent`, `ConfigParse`, `AgentsConfigured`, `ProjectsRootExists`, `RepoPathExists`, `RepoIsGitRepo`, `WorkspaceMembersResolve`, `AgentDocPresent`) with `serde::Serialize` rendering the variant name in PascalCase; derive `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `PartialOrd`, `Ord` (alphabetical sort acceptable)
- [x] 1.4 Define `Finding { check: Check, severity: Severity, target: String, message: String }` and `Summary { ok: u32, warn: u32, error: u32, total: u32 }` with `serde::Serialize`; derive `Debug`, `Clone`
- [x] 1.5 Define `DoctorReport { schema_version: u32, generated_at: String, checks: Vec<Finding>, summary: Summary }` with `serde::Serialize`; add `pub const DOCTOR_SCHEMA_VERSION: u32 = 1` re-exported from the module
- [x] 1.6 Register the `doctor` module in `crates/repograph-core/src/lib.rs` and re-export `Check`, `Severity`, `Finding`, `Summary`, `DoctorReport`, `DOCTOR_SCHEMA_VERSION`
- [x] 1.7 Run `cargo check -p repograph-core` and `cargo clippy -p repograph-core -- -D warnings`; commit the skeleton

## 2. Core: per-check implementations

- [x] 2.1 Add `DoctorReport::run(config: Result<&Config, &RepographError>, config_path: &Path) -> DoctorReport`; the `Result` arg lets the caller surface `ConfigPresent` / `ConfigParse` failures cleanly (passing `Ok(&config)` on success, `Err(&load_err)` when the config load itself failed); the returned report always includes a synthetic `ConfigPresent` or `ConfigParse` finding plus subsequent checks (skipped when the config didn't load)
- [x] 2.2 Implement `Check::ConfigPresent`: emit an `Ok` finding when the config file exists at `config_path`; emit an `Error` finding (with `target = config_path.display().to_string()`) when it does not — and short-circuit the rest of the catalog
- [x] 2.3 Implement `Check::ConfigParse`: emit an `Ok` finding when the config parsed; emit an `Error` finding naming the parse error when it did not — and short-circuit the rest of the catalog
- [x] 2.4 Implement `Check::AgentsConfigured`: emit an `Ok` finding when `config.agents().is_some()` (or whatever the existing accessor on `Config` is for the `[agents]` section); emit a `Warn` finding (with `target = config_path.display().to_string()`) when the section is missing; gates `Check::AgentDocPresent`
- [x] 2.5 Implement `Check::ProjectsRootExists`: when `config.settings.projects_root` is `None`, emit nothing; when it is `Some(path)`, emit `Ok` if `path` exists as a directory, otherwise `Warn` naming the missing path
- [x] 2.6 Implement `Check::RepoPathExists` per repo: emit `Ok` when `repo.path` exists on disk, `Error` otherwise; gates `Check::RepoIsGitRepo` for that repo
- [x] 2.7 Implement `Check::RepoIsGitRepo` per repo (only when `Check::RepoPathExists` passed for the same repo): call `repograph_core::git::validate_git_repo(&repo.path)`; emit `Ok` on success, `Error` with the helper's error message on failure — no direct `git2::Repository::open` in `doctor.rs`
- [x] 2.8 Implement `Check::WorkspaceMembersResolve` per workspace: for each `member` name in the workspace, emit `Ok` when it resolves against `config.repos()`, `Warn` (one finding per dangling member) when it does not; `target = "<workspace-name>"` for warn findings, `target = "<workspace-name> / <repo-name>"` for ok findings (or pick a single convention — document it on the `Finding` doc comment)
- [x] 2.9 Implement `Check::AgentDocPresent` (only when `Check::AgentsConfigured` passed AND the agents list is non-empty): for each registered repo × each agent in `config.agents().selected`, call `repograph_core::context::resolve_agent_docs(&repo.path, &[agent])`; emit `Ok` when the returned `Vec<AgentDoc>` has at least one entry with `files.len() > 0`, `Warn` otherwise; `target = "<repo-name> / <agent-id>"`
- [x] 2.10 Parallelize per-repo checks via `rayon::iter::IntoParallelIterator` over `config.repos()` (each per-repo iteration produces its own `Vec<Finding>`; flatten post-collection); workspace and config-level checks run sequentially in the same `DoctorReport::run` body
- [x] 2.11 After collecting all findings, sort by `(severity DESC, check ASC, target ASC)` using the derived `Ord` impls (reverse on severity); compute `Summary` from the sorted list; populate `generated_at` with an RFC 3339 UTC timestamp (use the `time` crate already in the binary's deps — or, if simpler, generate the timestamp at the binary boundary and pass it in; revisit if needed)
- [x] 2.12 Add unit tests in `doctor.rs` covering each `Check` variant with `tempdir` fixtures: missing config file (`ConfigPresent` error), malformed TOML (`ConfigParse` error), config without `[agents]` (`AgentsConfigured` warn), config with `[settings].projects_root` pointing at a missing dir (`ProjectsRootExists` warn), repo with deleted path (`RepoPathExists` error), repo path exists but not a git repo (`RepoIsGitRepo` error), workspace with a dangling member (`WorkspaceMembersResolve` warn), repo missing the agent doc for a selected agent (`AgentDocPresent` warn); verify summary counts match the findings count per severity
- [x] 2.13 Run `cargo test -p repograph-core`; commit the check implementations

## 3. Binary: dependencies & clap wiring

- [x] 3.1 Add `clap_complete = "4"` to `crates/repograph/Cargo.toml` `[dependencies]`; pin to the same `clap` major version already in use
- [x] 3.2 Promote the `Cli` struct in `crates/repograph/src/main.rs` to `pub(crate)` (or expose it via a small `pub(crate) fn cli_command() -> clap::Command` helper) so `commands/completions.rs` can call `<Cli as clap::CommandFactory>::command()`
- [x] 3.3 Create `crates/repograph/src/commands/switch.rs` with `Args { name: String }` (clap derive) and a stubbed `run(args: &Args, config_dir: &Path) -> Result<(), RepographError>` returning `Ok(())`; add the `#[tracing::instrument(skip(args), fields(name = %args.name))]` attribute
- [x] 3.4 Create `crates/repograph/src/commands/completions.rs` with `Args { shell: clap_complete::Shell }` (clap derive; positional arg) and a stubbed `run(args: &Args) -> Result<(), RepographError>` returning `Ok(())`; add the `#[tracing::instrument(skip(args), fields(shell = ?args.shell))]` attribute
- [x] 3.5 Create `crates/repograph/src/commands/doctor.rs` with `Args { json: bool }` (clap derive; `#[arg(long)]`) and a stubbed `run(args: &Args, config_dir: &Path) -> Result<(), RepographError>` returning `Ok(())`; add the `#[tracing::instrument(skip(args), fields(json = args.json))]` attribute
- [x] 3.6 Register the three new modules in `crates/repograph/src/commands/mod.rs`
- [x] 3.7 Add three new `Command` variants (`Switch`, `Completions`, `Doctor`) to the enum in `main.rs` with doc comments matching the spec one-liners; wire dispatch arms in `main()`
- [x] 3.8 Run `cargo check` and `cargo clippy -- -D warnings`; commit the skeleton

## 4. Binary: switch command body

- [x] 4.1 Implement `switch::run`: load the config from `config_dir`; look up `args.name` against `config.repos()`; on miss, compute Levenshtein distance against every registered repo name (a small `fn levenshtein(a: &str, b: &str) -> usize` lives in `switch.rs` — 30 lines, no new dep), filter by `dist <= 2 && dist <= (args.name.len() / 2)`, take up to three sorted by `(distance ASC, name ASC)`, format a "did you mean: a, b, c?" string and emit it via `tracing::error!` (or a dedicated `eprintln!` if `tracing` re-flowing the line breaks the eval contract test — re-validate after wiring), then return `RepographError::NotFound { kind: "repo", name: args.name.clone() }`
- [x] 4.2 On hit, format the `cd <path>` line using a small `fn shell_quote(path: &Path) -> String` helper that wraps the path in single quotes (with `'\''` escaping) when it contains any character in `[ \t\n'"$\\\`*?[\]{}();&|<>!#~]`, otherwise emits unquoted; write `cd {quoted}\n` to `io::stdout()` via `writeln!` (NOT through `output.rs` — the line is structured-but-not-tabular and a renderer would over-engineer it)
- [x] 4.3 Add `#[tracing::instrument]` entry/success logs per the logging rule: `debug!(command = "switch", name = %args.name, "start")` already on the instrument; `info!(repo = %args.name, path = %repo.path.display(), "resolved")` on success
- [x] 4.4 Unit-test `shell_quote` with `tempdir` fixtures: plain ASCII path → unquoted; path with space → single-quoted; path with embedded `'` → `'\''`-escaped; path with `$` → single-quoted; path with tilde → single-quoted (since `~` only expands at the shell's argument-parsing stage, but we conservatively quote it)
- [x] 4.5 Unit-test the `levenshtein` helper against a small fixture (empty / one-edit / two-edit / unrelated strings)
- [x] 4.6 Unit-test the suggestion filter: list of names with one near-miss returns it; with no near-miss returns empty; ties order by name; result truncated to three
- [x] 4.7 Run `cargo check` / `cargo clippy -- -D warnings`; commit the switch body

## 5. Binary: completions command body

- [x] 5.1 Implement `completions::run`: obtain the `clap::Command` AST via `<crate::Cli as clap::CommandFactory>::command()` (or the helper from task 3.2); call `clap_complete::generate(args.shell, &mut cmd, "repograph", &mut io::stdout())`; return `Ok(())`
- [x] 5.2 Add the `info!(shell = ?args.shell, "completions generated")` log on success
- [x] 5.3 Sanity-check by running `cargo run -- completions fish` and `cargo run -- completions bash` locally; eyeball the output for the canonical markers (`complete -c repograph` for fish, `_repograph()` for bash)
- [x] 5.4 Run `cargo check` / `cargo clippy -- -D warnings`; commit the completions body

## 6. Binary: doctor command body & rendering

- [x] 6.1 In `crates/repograph/src/output.rs`, add `render_doctor_json(report: &DoctorReport, stdout: &mut impl Write) -> io::Result<()>` that writes `serde_json::to_writer(stdout, report)` (single-line; no trailing newline); unit-test the bytes round-trip into a `serde_json::Value` whose `schema_version` is `1` and whose `checks` array length equals `summary.total`
- [x] 6.2 Add `render_doctor_table(report: &DoctorReport, stdout: &mut impl Write) -> io::Result<()>` using `comfy-table::Table::new()` with `load_preset(comfy_table::presets::UTF8_FULL)`; columns `Severity | Check | Target | Message`; rows iterate `report.checks` in the already-sorted order; colorize the `Severity` cell via `console::Style` (`error` red, `warn` yellow, `ok` green); follow the table with a single footer line `<N> ok · <M> warn · <K> error` with matching color treatment
- [x] 6.3 Implement `doctor::run`: compute `OutputMode` once from `is_terminal::IsTerminal` on stdout and `args.json`; attempt to load the config — on `Err(RepographError::Io(e))` where `e.kind() == PermissionDenied`, propagate (exit `4`); on any other `Err`, capture as the input to `DoctorReport::run` (the report will surface it as a `ConfigParse` / `ConfigPresent` error finding); on `Ok(config)`, pass `Ok(&config)` to `DoctorReport::run`
- [x] 6.4 Compute `config_path = config_dir.join(repograph_core::CONFIG_FILE_NAME)` and pass it to `DoctorReport::run` for the `ConfigPresent` finding's `target`
- [x] 6.5 Dispatch: `match output_mode { Json | NonTty => render_doctor_json, Tty => render_doctor_table }`; after rendering, if `report.summary.error > 0` and `output_mode == Tty`, emit a hint on stderr via `tracing::info!` (or `eprintln!`) suggesting `run 'repograph doctor --json | jq' for machine-readable detail`
- [x] 6.6 Determine the exit by computing the exit code locally: `report.summary.error > 0 → return RepographError::DoctorErrorsFound` (a new variant mapped to exit `1`) — OR, simpler, return `Ok(())` on no-error and a `RepographError::UsageError(...)` already mapped to exit `1` on any error. Decide between adding a dedicated variant vs reusing `UsageError`; document the choice in `design.md`'s Resolved deviations if it deviates from the spec wording, and ensure the `tracing::error!` line in `main::report` doesn't print a misleading "repograph failed" banner when the underlying issue is just doctor findings (consider suppressing `tracing::error!` for this specific code path)
- [x] 6.7 Add the success log: `info!(ok = report.summary.ok, warn = report.summary.warn, error = report.summary.error, total = report.summary.total, "doctor complete")`; do NOT emit per-finding `warn!` / `error!` lines (the spec forbids the duplication)
- [x] 6.8 Run `cargo check` / `cargo clippy -- -D warnings`; commit the doctor body and renderers

## 7. Acceptance tests (assert_cmd)

- [x] 7.1 Create `crates/repograph/tests/switch.rs` with a shared `tempdir` + `Config` fixture builder (reuse patterns from existing acceptance tests); each test sets `REPOGRAPH_CONFIG_DIR` to a `tempdir` to isolate from the real user config
- [x] 7.2 Test: successful switch — register a repo `api` at a `tempdir` path; run `repograph switch api`; assert stdout is exactly `cd <path>\n` (byte-precise; use `predicates::str::eq` against the expected string); exit `0`
- [x] 7.3 Test: path-with-space quoting — register a repo at `<tempdir>/has space/repo`; assert stdout is `cd '<full path>'\n`
- [x] 7.4 Test: path-with-quote escaping — register a repo at `<tempdir>/mike's repo` (where the FS allows it; skip on Windows); assert stdout uses the `'\''` escape sequence
- [x] 7.5 Test: stdout-only contract — pipe `repograph switch api 2>/dev/null` to a file; assert the file contents are exactly the one line and nothing else (no banner / log leak)
- [x] 7.6 Test: unknown name exits 3 with empty stdout — `repograph switch nope`; assert exit `3`, stdout zero bytes, stderr contains `nope`
- [x] 7.7 Test: near-miss suggestion — register `api`; run `repograph switch app`; assert stderr contains `did you mean: api`; exit `3`
- [x] 7.8 Test: no near-miss → no suggestion — register only `api`; run `repograph switch zzzz`; assert stderr does NOT contain `did you mean`
- [x] 7.9 Test: works without `[agents]` — config has no `[agents]` section; `repograph switch api` succeeds with the `cd <path>` line; no `NeedsInit` is raised
- [x] 7.10 Create `crates/repograph/tests/completions.rs`
- [x] 7.11 Test (parametric over shell): `repograph completions <shell>` for each of `bash`, `zsh`, `fish`, `powershell`, `elvish`; assert exit `0` and stdout contains the canonical marker per shell (`_repograph()` for bash, `#compdef repograph` for zsh, `complete -c repograph` for fish, `Register-ArgumentCompleter` for powershell, `edit:completion` for elvish)
- [x] 7.12 Test: unknown shell — `repograph completions tcsh`; assert exit `2` (clap usage), stdout empty, stderr contains a value-error message
- [x] 7.13 Test: completion reflects subcommand surface — `repograph completions bash`; assert stdout contains the literal substrings `switch`, `completions`, `doctor`, `context`, `list`, `add`, `remove`, `status`, `init`, `workspace`
- [x] 7.14 Create `crates/repograph/tests/doctor.rs`
- [x] 7.15 Test: clean config — register two repos (both real `git2`-initialized `tempdir` repos), one workspace with both as live members, `[agents].selected = ["claude-code"]` and both repos contain `CLAUDE.md`; run `repograph doctor --json`; parse JSON; assert `schema_version == 1`, `summary.error == 0`, `summary.warn == 0`, `summary.ok > 0`; exit `0`
- [x] 7.16 Test: missing repo path — register `api` at a path that's been removed; `repograph doctor --json`; assert the report contains a `Check::RepoPathExists` `error` finding for `api`; exit `1`
- [x] 7.17 Test: non-git path — register `notes` at a `tempdir` that's not a git repo; assert the report contains an `ok` finding for `RepoPathExists` and an `error` finding for `RepoIsGitRepo`; exit `1`
- [x] 7.18 Test: dangling workspace member — register `api`; create workspace `acme` with `members = ["api", "ghost"]`; assert the report contains a `Check::WorkspaceMembersResolve` `warn` finding naming `ghost`; exit `0`
- [x] 7.19 Test: missing agent doc — register `api` with no `CLAUDE.md`; `[agents].selected = ["claude-code"]`; assert the report contains a `Check::AgentDocPresent` `warn` finding for `api / claude-code`; exit `0`
- [x] 7.20 Test: missing `[agents]` section — config without `[agents]`; assert the report contains a `Check::AgentsConfigured` `warn` finding; assert NO `Check::AgentDocPresent` findings are present (the gate held); exit `0`
- [x] 7.21 Test: missing config file — point `REPOGRAPH_CONFIG_DIR` at a `tempdir` with no `config.toml`; assert the report contains an `error` finding for `Check::ConfigPresent`; exit `1`; stdout still emits the JSON envelope
- [x] 7.22 Test: config permission-denied — `chmod 000` on the config file (gated `cfg!(unix)`); assert exit `4`, stdout empty, stderr contains a permission-denied message
- [x] 7.23 Test: stdout-only contract — `repograph doctor --json 2>/dev/null`; pipe to file; assert the file parses as a single JSON object with no leading/trailing log lines
- [x] 7.24 Test: JSON sort order — produce a report with one error + two warns + three oks; assert the `checks` array's severities are in `error, warn, warn, ok, ok, ok` order
- [x] 7.25 Test: summary totals match — assert `summary.total == summary.ok + summary.warn + summary.error` and equals `checks.len()`
- [x] 7.26 Test: TTY-mode table rendering — invoke a thin wrapper that calls `render_doctor_table` directly against a synthetic `DoctorReport` (since `assert_cmd` strips the TTY); assert the rendered string contains the expected column headers (`Severity`, `Check`, `Target`, `Message`) and the footer line `<N> ok · <M> warn · <K> error`
- [x] 7.27 Test: error-only hint appears on stderr in TTY rendering — synthesize a report with one error; invoke the renderer wrapper plus the stderr hint helper; assert the hint string contains `repograph doctor --json | jq`
- [x] 7.28 Run `cargo test --workspace`; iterate until green; commit the test suite

## 8. README & docs

- [x] 8.1 Add three new rows to the `README.md` command table: `repograph switch <name>`, `repograph completions <shell>`, `repograph doctor [--json]` with one-line descriptions matching the spec
- [x] 8.2 Add a `### Shell integration` subsection covering: the `rg-cd` shell function for bash/zsh and fish (verbatim snippets from the spec), the one-time completion install commands for each supported shell (the five commands from the `shell-integration` spec's README requirement), a note that `switch` does not validate path existence and `doctor` is the validity-check tool
- [x] 8.3 Add a `### Doctor` subsection with: the full check catalog table (one row per `Check` variant with what it verifies and the severity on fail), the JSON envelope example (matching the `doctor-command` spec's documented shape, pretty-printed for legibility with a note that actual stdout is single-line), the exit-code mapping (`0` clean / `1` error / `4` permission-denied), an explicit "read-only and zero-network" note
- [x] 8.4 Update the exit-code table at the bottom of the README if needed (no new codes expected; verify against the existing table for accuracy)
- [x] 8.5 Update `CLAUDE.md`'s Manual Validation Checklist if the `switch` and `doctor` lines need rephrasing now that the behavior is concrete (current entries match the contract — likely no change needed; verify)

## 9. Final checks & archive readiness

- [x] 9.1 `cargo build --release` succeeds; binary runs all three new commands end-to-end
- [x] 9.2 `cargo test --workspace` is green
- [x] 9.3 `cargo clippy --workspace -- -D warnings` is clean
- [x] 9.4 `cargo fmt --all --check` is clean
- [x] 9.5 Run `repograph switch <real-repo> | head -c 200` against a real config and confirm the line is shell-eval-safe; `eval "$(repograph switch <real-repo>)"` in an interactive shell and confirm the cwd changes
- [x] 9.6 Run `repograph completions fish > /tmp/repograph.fish` and source it in fish; confirm tab completion lists the subcommands; repeat the smoke test for at least one of bash/zsh
- [x] 9.7 Run `repograph doctor --json | jq .` against a real config and confirm parseable output; eyeball the findings against the actual state of the registered repos
- [x] 9.8 Run `repograph doctor` in a TTY and visually confirm the table renders cleanly with color; verify the footer line and the error-only stderr hint behavior by deleting a registered repo and re-running
- [x] 9.9 Run `repograph doctor > out.json` (non-TTY) and confirm `out.json` is JSON, not a table
- [x] 9.10 Run `openspec validate shell-integration --type change --strict` — must be green
- [x] 9.11 Update `design.md` with any resolved deviations from the original plan (per `documentation.md`)
- [x] 9.12 Tick every task above; commit; ready for `/opsx:archive`
