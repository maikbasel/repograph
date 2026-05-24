## 1. Core: dependencies & module skeleton

- [x] 1.1 Add `globset` to `crates/repograph-core/Cargo.toml` (workspace dependency); add `chrono` with `serde` feature for the `generated_at` RFC 3339 timestamp (or reuse an existing time crate if one is already in the dependency closure)
- [x] 1.2 Create `crates/repograph-core/src/context.rs` with module-level doc comment, public types stubbed only enough to wire the module into `lib.rs` and let `cargo check` pass
- [x] 1.3 Register the new `context` module in `crates/repograph-core/src/lib.rs` with public re-exports (`Context`, `RepoContext`, `AgentDoc`, `MatchedFile`, `Scope`)
- [x] 1.4 Run `cargo check` and `cargo clippy -- -D warnings`; commit the skeleton

## 2. Core: pattern resolution engine

- [x] 2.1 Define `AgentDoc { agent: AgentId, files: Vec<MatchedFile> }` and `MatchedFile { path: PathBuf, bytes: u64, content: String }` in `context.rs`; derive `serde::Serialize`, `Debug`, `Clone`; ensure `MatchedFile.path` serializes as the **relative** forward-slash path (custom serializer if needed)
- [x] 2.2 Add an internal helper that, for a slice of `&'static str` patterns, splits them into (a) flat-file patterns (no glob metacharacters, no path separator beyond a known parent dir like `.github/copilot-instructions.md`) and (b) glob patterns under a known parent directory (e.g. `.cursor/rules/*.md`). Justify the classification with a small unit test enumerating every v1 registry pattern.
- [x] 2.3 Implement `resolve_agent_docs(repo_root: &Path, agents: &[AgentId]) -> (Vec<AgentDoc>, Vec<String>)` returning per-agent matched-file blocks and a flat list of warning strings collected during resolution; never panics, never `unwrap`s
- [x] 2.4 For flat patterns: use `fs_err::metadata` to check existence; read file via `fs_err::read_to_string`; on UTF-8 failure push a warning and skip the file; on permission / I/O failure push a warning and skip the file
- [x] 2.5 For glob patterns: list the known parent dir non-recursively via `fs_err::read_dir`; compile patterns into a `GlobSet`; for each entry match against the set; same read / UTF-8 / error handling as flat patterns
- [x] 2.6 Dedupe matched files within a single agent's `files` array by canonical path; sort each agent's `files` by relative path ascending
- [x] 2.7 Unit-test `resolve_agent_docs` with `tempdir` fixtures: empty repo (no files match), one flat file, one glob match, mixed flat + glob under same agent, unreadable file (chmod 000 — gated on non-Windows), non-UTF-8 file (write `&[0xFF, 0xFE]`), nested-but-not-root `CLAUDE.md` (should not match), repo with deep `node_modules` to prove we don't walk into it
- [x] 2.8 Run `cargo test -p repograph-core`; commit the resolver

## 3. Core: Context aggregator

- [x] 3.1 Define `RepoContext { name: String, path: PathBuf, branch: Option<String>, agent_docs: Vec<AgentDoc>, warnings: Vec<String> }` with `serde::Serialize`; serialize `path` as the canonical absolute path
- [x] 3.2 Define `Scope` enum with serde-tagged JSON shape matching the spec: `All`, `Workspace { name: String }`, `Repos { repos: Vec<String> }` — verify the tag layout serializes as `{ "kind": "all" }`, `{ "kind": "workspace", "name": "..." }`, `{ "kind": "repos", "repos": [...] }` via a unit test
- [x] 3.3 Define `Context { schema_version: u32, generated_at: String, agents: Vec<AgentId>, scope: Scope, repos: Vec<RepoContext>, warnings: Vec<String> }` with `serde::Serialize`; `schema_version` defaults to `1`; `generated_at` is RFC 3339 UTC
- [x] 3.4 Implement `Context::build(config: &Config, scope: Scope) -> Result<Context, RepographError>` that resolves the scope to a `Vec<&Repo>`, opens each repo via `git2::Repository::open` to read the current branch (mapping unborn / detached / bare / missing to `None` per the git-status spec — reuse the existing `git.rs` helper if one fits, otherwise add a narrow helper), runs `resolve_agent_docs` per repo, collects per-repo warnings, sorts `repos` by name ascending, and returns the assembled `Context`
- [x] 3.5 Add scope-resolution helpers: `Scope::resolve<'a>(&self, config: &'a Config) -> Result<Vec<&'a Repo>, RepographError>` returning `WorkspaceNotFound` or `RepoNotFound` (mapped to exit `3`) on unknown names; for `Scope::All`, return all repos sorted by name
- [x] 3.6 Add `RepographError::WorkspaceNotFound { name: String }` and `RepographError::RepoNotFound { name: String }` if they don't already exist; map both to exit code `3`; ensure existing exit-code mapping function covers them
- [x] 3.7 Parallelize per-repo work in `Context::build` via `rayon::iter::IntoParallelIterator` over the resolved repo slice; the work per repo is (open git, read branch, resolve agent docs, collect warnings); collect into a `Vec<RepoContext>` then sort by name
- [x] 3.8 Unit-test `Context::build` end-to-end with `tempdir` + real `git2`-initialized repos: default scope, workspace scope, repos scope; one repo with a CLAUDE.md; one repo with a missing path; one repo with a detached HEAD; verify `repos` sorted, `agent_docs` order preserved against `agents` input, warnings inline
- [x] 3.9 Run `cargo test -p repograph-core`; commit the aggregator

## 4. Binary: clap subcommand wiring

- [x] 4.1 Create `crates/repograph/src/commands/context.rs` with `Args` struct (derive `clap::Args`): `--workspace <NAME>` and `[REPOS]...` mutually exclusive via `conflicts_with`; `--json` flag; doc comments describing each
- [x] 4.2 Register the subcommand in `crates/repograph/src/commands/mod.rs` and wire dispatch in `main.rs`
- [x] 4.3 Stub `run(args: Args) -> Result<(), RepographError>` returning `Ok(())` so `cargo check` passes; add the `#[tracing::instrument(skip(args), fields(scope_kind = ?args.scope_kind()))]` attribute
- [x] 4.4 Run `cargo check` and `cargo clippy -- -D warnings`; commit the skeleton

## 5. Binary: command body

- [x] 5.1 In `run`: load config from `config_dir()`; call `ensure_agents_configured(&mut config, &config_dir)?`; on `NeedsInit` the existing error mapping handles exit `2`
- [x] 5.2 Derive `Scope` from `Args` (workspace → `Scope::Workspace`, repos → `Scope::Repos`, neither → `Scope::All`); resolve the scope through `Scope::resolve` to validate name existence before building the `Context` (so unknown-name errors fire before file I/O)
- [x] 5.3 Call `Context::build(&config, scope)`; on `Err` propagate (`?`); on `Ok(context)` proceed to render
- [x] 5.4 Compute `OutputMode` once at the top of `run` from `is_terminal::IsTerminal` on stdout AND the `--json` flag; pass it to the renderer
- [x] 5.5 Log entry at `debug`: `debug!(command = "context", scope_kind = ?scope.kind(), "start")`; log success at `info`: `info!(repos = context.repos.len(), agents = context.agents.len(), bytes = total_bytes, "context built")`; log per-repo / per-file warnings at `warn` with structured fields naming repo and path
- [x] 5.6 Run `cargo check` and `cargo clippy -- -D warnings`; commit the command body

## 6. Binary: rendering (JSON + Markdown)

- [x] 6.1 In `crates/repograph/src/output.rs`, add `render_context_json(context: &Context, stdout: &mut impl Write) -> io::Result<()>` that writes `serde_json::to_writer(stdout, context)` (single-line; no trailing newline) — verify with a unit test that the bytes parse back into a `serde_json::Value` whose `schema_version` is `1`
- [x] 6.2 Add `render_context_markdown(context: &Context, stdout: &mut impl Write) -> io::Result<()>` rendering per the spec: title naming scope + counts; `## <name>  (branch: <b>)` per repo with inline-code path on next line; `### <agent>` per agent; `#### <relpath> (<human-size>)` per file; fenced code block with content
- [x] 6.3 Implement fence-collision handling in the Markdown renderer: scan file content for `"```"` line; if present, use `~~~` fences; else use ```` ``` ```` fences — unit-test both branches with fixtures
- [x] 6.4 Render per-repo and global warnings as `> **warning:** <text>` blockquote lines below the appropriate heading; if a repo has missing path, render the repo section with no `### <agent>` subheadings, only the warning
- [x] 6.5 Implement a small `human_size(bytes: u64) -> String` helper rendering `"1.2 KB"`, `"567 B"`, etc.; unit-test edge cases (0, 999, 1024, 1024*1024)
- [x] 6.6 Dispatch in `run`: `match output_mode { Json | NonTty => render_context_json, Tty => render_context_markdown }`; flush after writing
- [x] 6.7 Run `cargo check` / `cargo clippy -- -D warnings`; commit the rendering

## 7. Acceptance tests (assert_cmd)

- [x] 7.1 Create `crates/repograph/tests/context.rs` with shared `tempdir` + `Config` fixture builder (reusing patterns from existing acceptance tests for `init`, `status`); each test sets `REPOGRAPH_CONFIG_DIR` to a `tempdir` to isolate from the real user config
- [x] 7.2 Test: default scope JSON happy path — two `git2`-initialized repos with `CLAUDE.md`, `--json` payload parses, `schema_version == 1`, both repos present, both `claude-code` `files` arrays contain `CLAUDE.md` with verbatim content
- [x] 7.3 Test: workspace scope — three repos, one workspace with two members; `--workspace <name> --json` payload contains exactly the two member repos; `scope.kind == "workspace"` and `scope.name` is set
- [x] 7.4 Test: positional scope — three repos, run with two named positionals; payload contains exactly those two; `scope.kind == "repos"` and `scope.repos` echoes user order; third repo absent
- [x] 7.5 Test: workspace + positional mutual exclusion — exit code `2`, stderr contains a clap usage message, stdout empty
- [x] 7.6 Test: unknown workspace — exit `3`, stderr names the workspace, stdout empty
- [x] 7.7 Test: unknown positional repo — exit `3`, stderr names the repo, stdout empty (no partial payload)
- [x] 7.8 Test: missing repo path — registered repo whose dir is removed before invocation; payload includes the entry with `branch: null`, `agent_docs: []`, `warnings` non-empty; exit `0`
- [x] 7.9 Test: unreadable file — `chmod 000` on a `CLAUDE.md` (gated `cfg!(unix)`); payload omits the file from `files` and includes a per-repo warning; exit `0`
- [x] 7.10 Test: non-UTF-8 file — write `&[0xFF, 0xFE]` as `.cursorrules`, select `cursor`; payload omits the file and includes a warning; exit `0`
- [x] 7.11 Test: glob expansion — repo with `.cursor/rules/a.md` and `.cursor/rules/b.md`, select `cursor`; payload contains both files sorted alphabetically
- [x] 7.12 Test: empty `[agents] selected = []` — payload's `agent_docs` arrays are empty per repo; exit `0`
- [x] 7.13 Test: non-TTY without `[agents]` (no `--json` flag either) — exit `2`, stderr names `repograph init`, stdout empty (`assert_cmd` runs without a TTY by default, so this is the default invocation)
- [x] 7.14 Test: stdout is byte-clean — run with `--json`, capture stdout, parse as JSON; assert no leading / trailing log lines bled into stdout
- [x] 7.15 Test: TTY-mode Markdown rendering — invoke a thin wrapper that calls `render_context_markdown` directly against a synthetic `Context` (since `assert_cmd` strips the TTY); assert the rendered string contains the expected headings, fences, and content
- [x] 7.16 Test: triple-backtick fence collision — synthesize a `Context` whose `MatchedFile.content` contains ` ``` `; render Markdown; assert the surrounding fences are `~~~`
- [x] 7.17 Test: large file (50 KB) — verify content is verbatim and length-preserved through both JSON and Markdown
- [x] 7.18 Run `cargo test --workspace`; iterate until green; commit the test suite

## 8. README & docs

- [x] 8.1 Add a `repograph context` row to the README's command table with a one-line description
- [x] 8.2 Add a `## context` subsection (or equivalent) with one example invocation per scope mode (default, `--workspace <name>`, positional `<repo>...`)
- [x] 8.3 Show a compact JSON payload example with all top-level fields (`schema_version`, `generated_at`, `agents`, `scope`, `repos`, `warnings`); show one `RepoContext` with one `AgentDoc` and one `MatchedFile` populated; pretty-printed in the README for legibility (note that the actual stdout is single-line)
- [x] 8.4 Show a short Markdown rendering example (the same data, TTY rendering)
- [x] 8.5 Add an exit-code note for `context` (uses `0/1/2/3/4`; `5` is not produced) — fits into the existing exit-code table without changes

## 9. Final checks & archive readiness

- [x] 9.1 `cargo build --release` succeeds; binary runs the new command end-to-end
- [x] 9.2 `cargo test --workspace` is green
- [x] 9.3 `cargo clippy --workspace -- -D warnings` is clean
- [x] 9.4 Run `repograph context --json | jq .` against a real config and confirm parseable output; capture the output, verify against the README example
- [x] 9.5 Run `repograph context` in a TTY and visually confirm the Markdown renders cleanly; copy-paste into a Markdown previewer (or render via `glow`) to confirm headers, code fences, blockquotes
- [x] 9.6 Run `repograph context > out.md` (non-TTY) and confirm `out.md` is JSON, not Markdown
- [x] 9.7 Run `openspec validate context-command --type change --strict` — must be green
- [x] 9.8 Update `design.md` with any resolved deviations from the original plan (per `documentation.md`)
- [x] 9.9 Tick every task above; commit; ready for `/opsx:archive`
