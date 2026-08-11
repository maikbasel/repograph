//! Acceptance tests for `repograph mcp serve`.
//!
//! Every test drives a real child process over a stdio pipe, exactly as an MCP
//! client does: JSON-RPC requests in on stdin, framed responses out on stdout.
//! Nothing here mocks the transport — the stdout-purity guarantee is only
//! meaningful when measured against the actual process.
//!
//! Requests are written as one batch and stdin is then closed; the server
//! answers each in turn and exits on EOF. That is enough to exercise
//! multi-call sessions, including "a failed call must not end the session".

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use serde_json::{Value, json};
use tempfile::TempDir;

use crate::common::{fixture_git_repo, fixture_git_repo_with_files, repograph_cmd};

/// The tool surface this change pins. A tool added or removed without updating
/// this list is a deliberate decision, and the closed-set test below forces it
/// to be made explicitly rather than drifting in.
const EXPECTED_TOOLS: [&str; 6] = [
    "repograph_context",
    "repograph_doctor",
    "repograph_find",
    "repograph_list",
    "repograph_status",
    "repograph_switch",
];

/// Upper bound on the serialised `tools/list` payload, in bytes.
///
/// Tool schemas are re-sent on every conversational turn, so this is a
/// recurring cost paid whether or not repograph is used. The ceiling is
/// deliberately close to the current size: raising it means accepting a larger
/// per-turn bill, which should be an explicit edit to this constant and not a
/// side effect of adding a parameter.
const SCHEMA_BYTE_CEILING: usize = 5_000;

/// Mutating command names that must never appear as MCP tools.
const FORBIDDEN_TOOL_FRAGMENTS: [&str; 6] = ["add", "edit", "remove", "workspace", "init", "index"];

fn rpc(id: u32, method: &str, params: &Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
}

fn initialize_line() -> String {
    rpc(
        1,
        "initialize",
        &json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "acceptance", "version": "1"}
        }),
    )
}

fn initialized_line() -> String {
    json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string()
}

/// Drive a `mcp serve` session with the given post-handshake request lines.
///
/// Returns `(responses_by_id, raw_stdout, stderr)`. Every stdout line is parsed
/// as JSON-RPC here, so a diagnostic leaking onto stdout fails every test that
/// uses this helper rather than only the one that looks for it.
fn serve_session(
    config_dir: &Path,
    requests: &[String],
) -> (std::collections::BTreeMap<u64, Value>, String, String) {
    let mut input = vec![initialize_line(), initialized_line()];
    input.extend_from_slice(requests);
    let stdin = format!("{}\n", input.join("\n"));

    let out = repograph_cmd(config_dir)
        .arg("mcp")
        .arg("serve")
        .write_stdin(stdin)
        .assert()
        .success();
    let output = out.get_output();
    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout is utf-8");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    let mut by_id = std::collections::BTreeMap::new();
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let value: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("stdout line is not JSON-RPC ({e}): {line}"));
        assert_eq!(
            value["jsonrpc"], "2.0",
            "every stdout frame carries the JSON-RPC version, got: {line}"
        );
        if let Some(id) = value.get("id").and_then(Value::as_u64) {
            by_id.insert(id, value);
        }
    }
    (by_id, stdout, stderr)
}

fn call_tool(id: u32, name: &str, arguments: &Value) -> String {
    rpc(
        id,
        "tools/call",
        &json!({"name": name, "arguments": arguments}),
    )
}

/// Configure an agent selection non-interactively. `context` refuses to run
/// without one, on both the CLI and the MCP surface.
///
/// `--scope user` with `HOME` redirected into the test's own tree keeps the
/// artifact writes hermetic — the real home directory is never touched.
fn init_agents(config_dir: &Path) {
    let home = config_dir
        .parent()
        .expect("config dir has a parent")
        .join("home");
    std::fs::create_dir_all(&home).unwrap();
    repograph_cmd(config_dir)
        .env("HOME", &home)
        .arg("init")
        .arg("--no-prompt")
        .arg("--agents")
        .arg("claude-code")
        .arg("--scope")
        .arg("user")
        .assert()
        .success();
}

fn add_repo(config_dir: &Path, repo_path: &Path, name: &str) {
    repograph_cmd(config_dir)
        .arg("add")
        .arg(repo_path)
        .arg("--name")
        .arg(name)
        .assert()
        .success();
}

#[test]
fn mcp_without_a_subcommand_prints_help_and_exits_2() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    repograph_cmd(&config_dir)
        .arg("mcp")
        .assert()
        .code(2)
        .stdout(predicates::str::is_empty());
}

#[test]
fn initialize_declares_repograph_identity_and_version() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let (by_id, _, _) = serve_session(&config_dir, &[]);

    let result = &by_id[&1]["result"];
    assert_eq!(result["serverInfo"]["name"], env!("CARGO_PKG_NAME"));
    assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(
        result["capabilities"].get("tools").is_some(),
        "server declares the tools capability, got: {}",
        result["capabilities"]
    );
}

#[test]
fn tools_list_returns_the_pinned_closed_set_all_read_only() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let (by_id, _, _) = serve_session(&config_dir, &[rpc(2, "tools/list", &json!({}))]);

    let tools = by_id[&2]["result"]["tools"].as_array().unwrap();
    let mut names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(
        names, EXPECTED_TOOLS,
        "tool set changed; update EXPECTED_TOOLS deliberately if intended"
    );

    for tool in tools {
        assert_eq!(
            tool["annotations"]["readOnlyHint"],
            json!(true),
            "{} must be annotated read-only so clients can auto-approve it",
            tool["name"]
        );
    }
}

#[test]
fn no_mutating_command_is_exposed_as_a_tool() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let (by_id, _, _) = serve_session(&config_dir, &[rpc(2, "tools/list", &json!({}))]);

    let tools = by_id[&2]["result"]["tools"].as_array().unwrap();
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        let bare = name.trim_start_matches("repograph_");
        assert!(
            !FORBIDDEN_TOOL_FRAGMENTS.contains(&bare),
            "mutating surface leaked into MCP as `{name}`"
        );
    }
}

#[test]
fn advertised_schema_stays_within_the_token_budget() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let (by_id, _, _) = serve_session(&config_dir, &[rpc(2, "tools/list", &json!({}))]);

    let serialized = serde_json::to_string(&by_id[&2]["result"]).unwrap();
    assert!(
        serialized.len() <= SCHEMA_BYTE_CEILING,
        "tools/list is {} bytes, over the {SCHEMA_BYTE_CEILING}-byte ceiling. \
         Tool schemas are re-sent every turn — shrink the descriptions or raise \
         SCHEMA_BYTE_CEILING deliberately.",
        serialized.len()
    );
}

#[test]
fn list_tool_output_is_identical_to_cli_json_output() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r1 = fixture_git_repo(tmp.path(), "alpha");
    let r2 = fixture_git_repo(tmp.path(), "beta");
    add_repo(&config_dir, &r1, "alpha");
    add_repo(&config_dir, &r2, "beta");

    let (by_id, _, _) = serve_session(&config_dir, &[call_tool(2, "repograph_list", &json!({}))]);
    let from_mcp = &by_id[&2]["result"]["structuredContent"];

    let cli = repograph_cmd(&config_dir).arg("list").assert().success();
    let from_cli: Value = serde_json::from_slice(&cli.get_output().stdout).unwrap();

    assert_eq!(
        *from_mcp, from_cli,
        "MCP tool result must be byte-equal to `list --json`"
    );
}

#[test]
fn status_tool_output_is_identical_to_cli_json_output() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r1 = fixture_git_repo(tmp.path(), "alpha");
    add_repo(&config_dir, &r1, "alpha");

    let (by_id, _, _) = serve_session(&config_dir, &[call_tool(2, "repograph_status", &json!({}))]);
    let from_mcp = &by_id[&2]["result"]["structuredContent"];

    let cli = repograph_cmd(&config_dir).arg("status").assert().success();
    let from_cli: Value = serde_json::from_slice(&cli.get_output().stdout).unwrap();

    assert_eq!(*from_mcp, from_cli);
}

/// Drop `generated_at` before comparing. `context` and `doctor` stamp the
/// moment they ran, so the two invocations being compared can never agree on
/// it; every other key must match exactly.
fn without_generated_at(mut value: Value) -> Value {
    if let Some(obj) = value.as_object_mut() {
        assert!(
            obj.remove("generated_at").is_some(),
            "expected a generated_at stamp to normalise away"
        );
    }
    value
}

#[test]
fn context_tool_output_is_identical_to_cli_json_output() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "alpha");
    add_repo(&config_dir, &repo, "alpha");
    init_agents(&config_dir);

    let (by_id, _, _) = serve_session(
        &config_dir,
        &[call_tool(2, "repograph_context", &json!({}))],
    );
    let from_mcp = by_id[&2]["result"]["structuredContent"].clone();

    let cli = repograph_cmd(&config_dir)
        .arg("context")
        .arg("--json")
        .assert()
        .success();
    let from_cli: Value = serde_json::from_slice(&cli.get_output().stdout).unwrap();

    assert_eq!(
        without_generated_at(from_mcp),
        without_generated_at(from_cli)
    );
}

#[test]
fn context_tool_refuses_to_run_without_a_configured_agent_selection() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "alpha");
    add_repo(&config_dir, &repo, "alpha");
    // Deliberately no `init_agents` — matching the CLI, which exits 2 here.

    let (by_id, _, _) = serve_session(
        &config_dir,
        &[call_tool(2, "repograph_context", &json!({}))],
    );
    let result = &by_id[&2]["result"];

    assert_eq!(
        result["isError"],
        json!(true),
        "an unconfigured registry must be a loud error, not an empty agent_docs payload"
    );
    let message = result["structuredContent"]["error"].as_str().unwrap();
    assert!(
        message.contains("repograph init"),
        "the error names the command that fixes it, got: {message}"
    );
}

#[test]
fn doctor_tool_output_is_identical_to_cli_json_output() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "alpha");
    add_repo(&config_dir, &repo, "alpha");

    let (by_id, _, _) = serve_session(&config_dir, &[call_tool(2, "repograph_doctor", &json!({}))]);
    let from_mcp = by_id[&2]["result"]["structuredContent"].clone();

    // `doctor` exits 1 when it finds any error-severity check, so the CLI side
    // is not asserted successful here — only that its payload matches.
    let cli = repograph_cmd(&config_dir)
        .arg("doctor")
        .arg("--json")
        .assert();
    let from_cli: Value = serde_json::from_slice(&cli.get_output().stdout).unwrap();

    assert_eq!(
        without_generated_at(from_mcp),
        without_generated_at(from_cli)
    );
}

#[test]
fn switch_tool_returns_a_path_not_a_shell_command() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "alpha");
    add_repo(&config_dir, &repo, "alpha");

    let (by_id, _, _) = serve_session(
        &config_dir,
        &[call_tool(2, "repograph_switch", &json!({"name": "alpha"}))],
    );

    let payload = &by_id[&2]["result"]["structuredContent"];
    assert_eq!(payload["name"], "alpha");
    assert_eq!(payload["path"], repo.to_string_lossy().as_ref());

    let rendered = payload.to_string();
    assert!(
        !rendered.contains("cd "),
        "MCP consumers are JSON-RPC clients, not shells; got: {rendered}"
    );
}

#[test]
fn a_failed_tool_call_leaves_the_session_usable() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "alpha");
    add_repo(&config_dir, &repo, "alpha");

    let (by_id, _, _) = serve_session(
        &config_dir,
        &[
            call_tool(2, "repograph_switch", &json!({"name": "nope"})),
            call_tool(3, "repograph_switch", &json!({"name": "alpha"})),
        ],
    );

    let failed = &by_id[&2]["result"];
    assert_eq!(
        failed["isError"],
        json!(true),
        "unknown repo is a tool-level error"
    );
    assert!(
        failed["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("nope"),
        "the error names the repo the caller asked for, got: {}",
        failed["structuredContent"]
    );
    assert_eq!(
        failed["structuredContent"]["exit_code"],
        json!(3),
        "not-found maps to the documented exit code 3"
    );

    assert_eq!(
        by_id[&3]["result"]["isError"],
        json!(false),
        "the session survived the failed call and answered the next one"
    );
}

#[test]
fn stdout_stays_pure_protocol_when_a_repo_path_is_missing() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "ghost");
    add_repo(&config_dir, &repo, "ghost");
    // Delete the repo out from under the registry: the paranoid path.
    std::fs::remove_dir_all(&repo).unwrap();

    // `serve_session` parses every stdout line as JSON-RPC, so a diagnostic
    // leaking onto stdout fails inside the helper.
    let (by_id, stdout, _stderr) = serve_session(
        &config_dir,
        &[
            call_tool(2, "repograph_status", &json!({})),
            call_tool(3, "repograph_list", &json!({})),
        ],
    );

    assert!(by_id.contains_key(&2) && by_id.contains_key(&3));
    assert!(
        !stdout.contains("WARN") && !stdout.contains("ERROR"),
        "tracing output must never reach stdout in serve mode"
    );
}

#[test]
fn find_tool_returns_lexical_hits_and_never_reports_semantic() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    // The file must be tracked — indexing is git-aware, so an untracked file is
    // invisible to `find` by design. The uncommitted *edit* to it is what the
    // pre-search refresh is meant to catch.
    let repo =
        fixture_git_repo_with_files(tmp.path(), "alpha", &[("auth.rs", "fn placeholder() {}\n")]);
    add_repo(&config_dir, &repo, "alpha");
    std::fs::write(
        repo.join("auth.rs"),
        "fn rotate_refresh_token() { /* jwt rotation lives here */ }\n",
    )
    .unwrap();

    // No explicit `repograph index` first: the auto-refresh must pick the edit
    // up, exactly as the CLI's `find` does.
    let (by_id, _, _) = serve_session(
        &config_dir,
        &[call_tool(
            2,
            "repograph_find",
            &json!({"query": "rotate_refresh_token"}),
        )],
    );

    let payload = &by_id[&2]["result"]["structuredContent"];
    assert_eq!(
        payload["semantic_used"],
        json!(false),
        "the MCP surface is lexical-only"
    );
    let hits = payload["hits"].as_array().unwrap();
    assert!(
        hits.iter().any(|h| h["repo"] == "alpha"),
        "uncommitted edit should be found after auto-refresh, got: {payload}"
    );
}

#[test]
fn find_tool_honours_the_skip_refresh_escape() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo_with_files(
        tmp.path(),
        "alpha",
        &[("seed.rs", "fn seeded_symbol() {}\n")],
    );
    add_repo(&config_dir, &repo, "alpha");

    // Index the committed state, then edit the tracked file. Only a refresh
    // would see the new symbol — which is exactly what this call opts out of.
    repograph_cmd(&config_dir).arg("index").assert().success();
    std::fs::write(repo.join("seed.rs"), "fn added_after_index() {}\n").unwrap();

    let (by_id, _, _) = serve_session(
        &config_dir,
        &[call_tool(
            2,
            "repograph_find",
            &json!({"query": "added_after_index", "no_refresh": true}),
        )],
    );

    let payload = &by_id[&2]["result"]["structuredContent"];
    assert_eq!(
        payload["hits"].as_array().unwrap().len(),
        0,
        "skip-refresh must query the index as-is, got: {payload}"
    );
}

#[test]
fn registry_edits_are_visible_without_restarting_the_server() {
    use std::io::{BufRead, BufReader, Write};

    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();

    // A streaming session: the batch helper writes all input before the process
    // starts, which cannot express "mutate the registry mid-session".
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin!("repograph"))
        .env("REPOGRAPH_CONFIG_DIR", &config_dir)
        .env("REPOGRAPH_DATA_DIR", &config_dir)
        .args(["mcp", "serve"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn mcp serve");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();

    writeln!(stdin, "{}", initialize_line()).unwrap();
    writeln!(stdin, "{}", initialized_line()).unwrap();
    stdin.flush().unwrap();
    stdout.read_line(&mut line).unwrap(); // initialize result

    // First call: registry is empty.
    writeln!(stdin, "{}", call_tool(2, "repograph_list", &json!({}))).unwrap();
    stdin.flush().unwrap();
    line.clear();
    stdout.read_line(&mut line).unwrap();
    let first: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(
        first["result"]["structuredContent"]["repos"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    // Mutate the registry out-of-band while the server is still connected.
    let repo = fixture_git_repo(tmp.path(), "late");
    add_repo(&config_dir, &repo, "late");

    writeln!(stdin, "{}", call_tool(3, "repograph_list", &json!({}))).unwrap();
    stdin.flush().unwrap();
    line.clear();
    stdout.read_line(&mut line).unwrap();
    let second: Value = serde_json::from_str(&line).unwrap();
    let repos = second["result"]["structuredContent"]["repos"]
        .as_array()
        .unwrap();
    assert_eq!(
        repos.len(),
        1,
        "config is re-read per call, so the new repo appears without a restart"
    );
    assert_eq!(repos[0]["name"], "late");

    drop(stdin);
    child.wait().unwrap();
}

#[test]
fn unknown_tool_name_is_reported_without_killing_the_server() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let (by_id, _, _) = serve_session(
        &config_dir,
        &[
            call_tool(2, "repograph_not_a_tool", &json!({})),
            rpc(3, "tools/list", &json!({})),
        ],
    );

    assert!(
        by_id.contains_key(&2),
        "the server answered the bad tool name instead of dropping the request"
    );
    assert!(
        by_id[&3].get("result").is_some(),
        "the session still served tools/list afterwards"
    );
}
