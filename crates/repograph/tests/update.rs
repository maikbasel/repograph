//! Acceptance tests for `repograph update` and the passive update notifier.
//!
//! Each spec scenario in
//! `openspec/changes/self-update/specs/self-update/spec.md` that can be
//! exercised without network access is represented below. The networked paths
//! (live version query, in-place upgrade) require a GitHub endpoint and a real
//! install receipt; covering them hermetically needs a mocked release server
//! (`REPOGRAPH_INSTALLER_GHE_BASE_URL` redirects axoupdater at a local mock) and
//! is tracked as follow-up — CI here stays zero-network by construction.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use crate::common::repograph_cmd;

/// A `repograph update …` command fully isolated from any real install receipt
/// and caches: `HOME`/`XDG_*` point at a fresh tempdir so axoupdater finds no
/// receipt (deterministic "defer to package manager", no network) and the
/// notifier cache cannot escape the sandbox.
fn isolated_update_cmd(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("repograph").expect("repograph binary built");
    cmd.env_remove("REPOGRAPH_CONFIG_DIR")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .arg("update");
    cmd
}

#[test]
fn check_with_no_receipt_defers_to_package_manager() {
    let home = TempDir::new().unwrap();
    isolated_update_cmd(home.path())
        .arg("--check")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(
            predicate::str::contains("brew upgrade repograph")
                .and(predicate::str::contains("cargo install repograph")),
        );
}

#[test]
fn update_with_no_receipt_defers_and_changes_nothing() {
    let home = TempDir::new().unwrap();
    isolated_update_cmd(home.path())
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("package manager"));
}

#[test]
fn update_help_documents_check_flag() {
    Command::cargo_bin("repograph")
        .unwrap()
        .arg("update")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--check"));
}

/// Under `assert_cmd` stdout is never a TTY, so the notifier's stdout-TTY gate
/// must suppress it: a normal command prints no "available" nudge on stderr and
/// returns promptly (no network check, no hang).
#[test]
fn notifier_suppressed_when_stdout_not_a_tty() {
    let config = TempDir::new().unwrap();
    repograph_cmd(config.path())
        .arg("list")
        .arg("--json")
        .assert()
        .success()
        .stderr(predicate::str::contains("available").not());
}

#[test]
fn notifier_suppressed_by_repograph_env_optout() {
    let config = TempDir::new().unwrap();
    repograph_cmd(config.path())
        .env("REPOGRAPH_NO_UPDATE_CHECK", "1")
        .arg("list")
        .arg("--json")
        .assert()
        .success()
        .stderr(predicate::str::contains("available").not());
}

#[test]
fn notifier_suppressed_by_cross_tool_env_optout() {
    let config = TempDir::new().unwrap();
    repograph_cmd(config.path())
        .env("NO_UPDATE_NOTIFIER", "1")
        .arg("list")
        .arg("--json")
        .assert()
        .success()
        .stderr(predicate::str::contains("available").not());
}
