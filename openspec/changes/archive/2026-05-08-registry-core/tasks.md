## 1. Foundations

- [x] 1.1 Add runtime dependencies to `crates/repograph-core/Cargo.toml`: `serde` (with derive), `toml`, `git2`, `thiserror`, `dirs`, `fs-err`, `tracing`, `serde_json`.
- [x] 1.2 Add runtime dependencies to `crates/repograph/Cargo.toml`: `clap` (derive), `comfy-table`, `is-terminal`, `tracing`, `tracing-subscriber` (with `env-filter`), `serde_json`, plus `repograph-core` (workspace).
- [x] 1.3 Add dev-dependencies to `crates/repograph/Cargo.toml`: `assert_cmd`, `predicates`, `tempfile`, `serde_json`, `git2`.
- [x] 1.4 Add dev-dependencies to `crates/repograph-core/Cargo.toml`: `tempfile`, `pretty_assertions` (optional, for diff-readable failures).
- [x] 1.5 Initialize `tracing-subscriber` once at the top of `crates/repograph/src/main.rs`: stderr writer, `EnvFilter::try_from_default_env()` falling back to `info`.

## 2. Acceptance test scaffolding (outside-in TDD — tests written first)

- [x] 2.1 Create `crates/repograph/tests/common/mod.rs` with helpers: `fixture_git_repo(parent: &Path) -> PathBuf` (uses `git2` to init a real repo with one commit), `repograph_cmd(config_dir: &Path) -> Command` (sets `REPOGRAPH_CONFIG_DIR` env, refuses to build without it), `repograph_cmd_with_flag(config_dir: &Path) -> Command` (passes `--config-dir <path>` instead, used to test the flag path), `parse_list_json(stdout: &[u8]) -> serde_json::Value`.
- [x] 2.2 Create `crates/repograph/tests/add.rs` covering every scenario in spec `Add registers a local git repository` (success-named, infer-name, canonicalization, description+stack, not-a-git-repo, nonexistent-path, name-conflict, path-conflict).
- [x] 2.3 Create `crates/repograph/tests/list.rs` covering every scenario in spec `List renders the registered repositories` (TTY-table, JSON-when-piped, JSON-flag, empty-JSON, empty-table, deterministic-ordering).
- [x] 2.4 Create `crates/repograph/tests/remove.rs` covering every scenario in spec `Remove deregisters a repository by name` (success, nonexistent).
- [x] 2.5 Create `crates/repograph/tests/output_contract.rs` covering `Output contract` and `Exit codes` scenarios (jq pipe, diagnostics-never-on-stdout, missing-arg-exit-2, perm-denied-exit-4, malformed-toml-exit-1).
- [x] 2.5a Create `crates/repograph/tests/config_dir.rs` covering the precedence scenarios from spec `Config persistence`: `--config-dir` overrides env var, env var honored when flag absent, `--config-dir` accepted on every subcommand (global flag).
- [x] 2.6 Create `crates/repograph-core/tests/config_roundtrip.rs` for `Config persistence` scenarios that don't need the binary (first-git-creates-dir, round-trip-stability, unknown-fields-tolerated, empty-registry-no-file).
- [x] 2.7 `cargo test` — confirm the suite compiles, runs, and fails on assertions (not on missing types/subcommands once 3.x stubs exist).

## 3. Core domain (`repograph-core`)

- [x] 3.1 `crates/repograph-core/src/error.rs` — define `RepographError` with variants `Io(#[from] std::io::Error)`, `ConfigParse(toml::de::Error)`, `ConfigWrite(toml::ser::Error)`, `GitOpen { path, source }`, `NotFound { kind, name }`, `Conflict { kind, name }`, `PermissionDenied { path }`, `UsageError(String)`. Add `pub fn exit_code(&self) -> u8` mapping each variant to the contract codes.
- [x] 3.2 `crates/repograph-core/src/config.rs` — define `Repo { path: PathBuf, description: Option<String>, stack: Vec<String> }` with serde derives, `#[serde(default, skip_serializing_if = "Option::is_none")]` and `#[serde(default, skip_serializing_if = "Vec::is_empty")]`. Define `Config { repos: BTreeMap<String, Repo> }` with `#[serde(default)]`.
- [x] 3.3 `config.rs` — `pub const CONFIG_FILE_NAME: &str = "config.toml";` and `Config::default_dir() -> Option<PathBuf>` returning `dirs::config_dir().map(|d| d.join("repograph"))`. **No env / CLI handling in core** — resolution lives in the binary (see 4.6a).
- [x] 3.4 `config.rs` — `Config::load(dir: &Path) -> Result<Config, RepographError>` reading `dir.join(CONFIG_FILE_NAME)` (missing file → `Config::default()`; malformed → `ConfigParse`).
- [x] 3.5 `config.rs` — `Config::save(&self, dir: &Path) -> Result<(), RepographError>` with atomic write (temp-file + rename via `fs-err`) to `dir.join(CONFIG_FILE_NAME)`; creates `dir` if missing.
- [x] 3.6 `config.rs` — `Config::add_repo(&mut self, name: String, repo: Repo) -> Result<(), RepographError>` enforcing both name uniqueness (HashMap key) and path uniqueness (linear scan; flag `Conflict`).
- [x] 3.7 `config.rs` — `Config::remove_repo(&mut self, name: &str) -> Result<Repo, RepographError>` returning `NotFound` when absent.
- [x] 3.8 `crates/repograph-core/src/git.rs` — `pub fn validate_git_repo(path: &Path) -> Result<PathBuf, RepographError>` that canonicalizes the path, calls `git2::Repository::open`, returns the canonicalized absolute path on success or `GitOpen { ... }` / `NotFound` otherwise.
- [x] 3.9 `crates/repograph-core/src/lib.rs` — re-export `RepographError`, `Config`, `Repo`, `validate_git_repo`. Keep `VERSION`.
- [x] 3.10 Inline `#[cfg(test)] mod tests` in `config.rs`: TOML round-trip, name conflict, path conflict, unknown-field tolerance, default-empty-config-on-missing-file.
- [x] 3.11 Inline `#[cfg(test)] mod tests` in `error.rs`: every variant maps to the documented exit code.

## 4. CLI binary (`repograph`)

- [x] 4.1 `crates/repograph/src/output.rs` — `OutputMode { Tty, Json }`, `OutputMode::detect(force_json: bool) -> Self` checking `is-terminal::IsTerminal::is_terminal(&io::stdout())`. `pub fn render_repos(mode: OutputMode, repos: &BTreeMap<String, Repo>) -> Result<(), RepographError>` writing to stdout: `comfy-table` with `UTF8_FULL` preset on TTY, `serde_json::to_writer` of `{ "repos": [...] }` envelope on JSON.
- [x] 4.2 `crates/repograph/src/commands/mod.rs` — declare `pub mod add; pub mod list; pub mod remove;`.
- [x] 4.3 `crates/repograph/src/commands/add.rs` — `Args { path: PathBuf, name: Option<String>, description: Option<String>, stack: Vec<String> }` (clap `value_delimiter = ','` on `stack`). `pub fn run(args: Args) -> Result<(), RepographError>`: load config, `validate_git_repo` to canonicalize and verify, derive name from path basename when omitted, call `Config::add_repo`, save, log on stderr via `tracing`.
- [x] 4.4 `crates/repograph/src/commands/list.rs` — `Args { json: bool }`. `pub fn run(args: Args) -> Result<(), RepographError>`: load config, `OutputMode::detect(args.json)`, `render_repos`.
- [x] 4.5 `crates/repograph/src/commands/remove.rs` — `Args { name: String }`. `pub fn run(args: Args) -> Result<(), RepographError>`: load, `Config::remove_repo`, save, log confirmation on stderr.
- [x] 4.6 `crates/repograph/src/main.rs` — replace stub. Top-level `Cli { #[arg(long, global = true, env = "REPOGRAPH_CONFIG_DIR", value_name = "PATH")] config_dir: Option<PathBuf>, #[command(subcommand)] command: Command }` and `enum Command { Add(add::Args), List(list::Args), Remove(remove::Args) }`. Init tracing, dispatch, on error log via `tracing::error!` and `process::exit(err.exit_code() as i32)`.
- [x] 4.6a `main.rs` — `fn resolve_config_dir(override_path: Option<PathBuf>) -> Result<PathBuf, RepographError>` that returns the override if set, else `Config::default_dir()`, else `Err(RepographError::UsageError("no config directory available; pass --config-dir or set REPOGRAPH_CONFIG_DIR"))` mapped to exit code `1`. Resolved once at dispatch; passed into each command's `run()` as `&Path`.
- [x] 4.6b Update `commands/{add,list,remove}.rs` `run()` signatures to accept `config_dir: &Path` alongside their `Args`.
- [x] 4.7 Add `#[tracing::instrument(skip(args))]` to each command's `run()` with relevant `fields(...)` (including `config_dir = %config_dir.display()`).

## 5. Verify spec coverage and gates

- [x] 5.1 Cross-check: every `#### Scenario:` in `specs/registry-core/spec.md` has at least one corresponding test (acceptance or unit). Add a tracking comment in tasks.md if any are intentionally deferred (target: zero deferrals).
- [x] 5.2 `cargo test` green across both crates.
- [x] 5.3 `cargo clippy --all-targets -- -D warnings` clean.
- [x] 5.4 `cargo check` clean with zero warnings.
- [x] 5.5 Manual smoke (documented in PR description): `repograph add . --name self`, `repograph list`, `repograph list --json | jq '.repos[0].name'`, `repograph remove self`, `repograph list --json` returns `{ "repos": [] }`.
- [x] 5.6 No `unwrap()` / `expect()` / `todo!()` / `unimplemented!()` outside of `#[cfg(test)]` blocks (grep gate).
- [x] 5.7 No `println!` / `eprintln!` outside of `output.rs` data renderers (grep gate; clap and tracing-subscriber emit their own output through their own channels).

## 6. Documentation

- [x] 6.1 Update `README.md`: add the command surface (`add`, `list`, `remove`), exit-code table, sample TOML, install-instructions placeholder pointing to Phase 7.
- [x] 6.2 Update `design.md` with any resolved deviations from the original decisions (e.g. canonicalization choice, unknown-field handling).
- [x] 6.3 `openspec validate registry-core` passes.
- [x] 6.4 Run through the archive checklist in `.claude/rules/documentation.md` before invoking `/opsx:archive`.
