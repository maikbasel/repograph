//! Acceptance tests for `repograph completions <shell>`.
//!
//! Each spec scenario in
//! `openspec/changes/shell-integration/specs/shell-integration/spec.md`
//! around `completions` is represented below.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use tempfile::TempDir;

use crate::common::repograph_cmd;

/// `completions` is config-independent, but the shared helper still wants a
/// config dir.
fn config_dir() -> TempDir {
    TempDir::new().unwrap()
}

fn run_completions(shell: &str) -> Vec<u8> {
    let tmp = config_dir();
    let out = repograph_cmd(tmp.path())
        .arg("completions")
        .arg(shell)
        .assert()
        .success();
    out.get_output().stdout.clone()
}

#[test]
fn fish_completions_contain_complete_directive() {
    let stdout = run_completions("fish");
    let s = String::from_utf8_lossy(&stdout);
    assert!(
        s.lines().any(|l| l.contains("complete -c repograph")),
        "fish completion has `complete -c repograph`: {s}"
    );
}

#[test]
fn bash_completions_contain_function_definition() {
    let stdout = run_completions("bash");
    let s = String::from_utf8_lossy(&stdout);
    assert!(
        s.contains("_repograph()") || s.contains("_repograph_main()"),
        "bash completion defines _repograph(): {s}"
    );
}

#[test]
fn zsh_completions_contain_compdef_directive() {
    let stdout = run_completions("zsh");
    let s = String::from_utf8_lossy(&stdout);
    assert!(
        s.contains("#compdef repograph"),
        "zsh completion has #compdef line: {s}"
    );
}

#[test]
fn powershell_completions_contain_register_argument_completer() {
    let stdout = run_completions("powershell");
    let s = String::from_utf8_lossy(&stdout);
    assert!(
        s.contains("Register-ArgumentCompleter"),
        "powershell completion has Register-ArgumentCompleter: {s}"
    );
}

#[test]
fn elvish_completions_contain_edit_completion_namespace() {
    let stdout = run_completions("elvish");
    let s = String::from_utf8_lossy(&stdout);
    assert!(
        s.contains("edit:completion"),
        "elvish completion references edit:completion: {s}"
    );
}

#[test]
fn unknown_shell_exits_2_with_empty_stdout() {
    let tmp = config_dir();
    let out = repograph_cmd(tmp.path())
        .arg("completions")
        .arg("tcsh")
        .assert()
        .code(2);
    assert!(
        out.get_output().stdout.is_empty(),
        "stdout empty on usage error"
    );
}

#[test]
fn bash_completion_reflects_live_subcommand_surface() {
    let stdout = run_completions("bash");
    let s = String::from_utf8_lossy(&stdout);
    for name in [
        "switch",
        "completions",
        "doctor",
        "context",
        "list",
        "add",
        "remove",
        "status",
        "init",
        "workspace",
    ] {
        assert!(
            s.contains(name),
            "bash completion lists subcommand `{name}`: {s}"
        );
    }
}
