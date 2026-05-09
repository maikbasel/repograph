//! Integration tests for `repograph_core::Config` persistence using real tempdirs.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use repograph_core::{Config, Repo};
use tempfile::TempDir;

#[test]
fn first_save_creates_directory_and_file() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("nested").join("config");

    let mut cfg = Config::default();
    cfg.add_repo(
        "foo".to_string(),
        Repo {
            path: PathBuf::from("/tmp/foo"),
            description: None,
            stack: vec![],
        },
    )
    .expect("first add");

    cfg.save(&dir).expect("save");
    assert!(dir.exists(), "directory created");
    assert!(dir.join("config.toml").exists(), "file created");
}

#[test]
fn round_trip_byte_stable() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("conf");

    let mut cfg = Config::default();
    cfg.add_repo(
        "alpha".to_string(),
        Repo {
            path: PathBuf::from("/tmp/alpha"),
            description: Some("hello".into()),
            stack: vec!["rust".into(), "cli".into()],
        },
    )
    .unwrap();
    cfg.add_repo(
        "beta".to_string(),
        Repo {
            path: PathBuf::from("/tmp/beta"),
            description: None,
            stack: vec![],
        },
    )
    .unwrap();

    cfg.save(&dir).unwrap();
    let first = std::fs::read(dir.join("config.toml")).unwrap();

    let reloaded = Config::load(&dir).unwrap();
    reloaded.save(&dir).unwrap();
    let second = std::fs::read(dir.join("config.toml")).unwrap();

    assert_eq!(first, second, "byte-identical round trip");
}

#[test]
fn unknown_field_is_tolerated_on_load() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("conf");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        r#"
[repo.foo]
path = "/tmp/foo"
unknown_future_field = "hi"
"#,
    )
    .unwrap();

    let cfg = Config::load(&dir).expect("load tolerates unknown field");
    assert!(cfg.repos().contains_key("foo"));
}

#[test]
fn missing_file_yields_empty_config_no_file_written() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("never-written");

    let cfg = Config::load(&dir).expect("missing file ok");
    assert!(cfg.repos().is_empty());
    assert!(
        !dir.join("config.toml").exists(),
        "load does not create the file"
    );
}

#[test]
fn name_conflict_returns_error_no_mutation() {
    let mut cfg = Config::default();
    cfg.add_repo(
        "foo".into(),
        Repo {
            path: PathBuf::from("/tmp/a"),
            description: None,
            stack: vec![],
        },
    )
    .unwrap();
    let err = cfg
        .add_repo(
            "foo".into(),
            Repo {
                path: PathBuf::from("/tmp/b"),
                description: None,
                stack: vec![],
            },
        )
        .unwrap_err();

    assert_eq!(err.exit_code(), 5, "conflict maps to exit 5");
    // First entry untouched.
    assert_eq!(
        cfg.repos().get("foo").unwrap().path,
        PathBuf::from("/tmp/a")
    );
}

#[test]
fn path_conflict_returns_error_no_mutation() {
    let mut cfg = Config::default();
    cfg.add_repo(
        "foo".into(),
        Repo {
            path: PathBuf::from("/tmp/shared"),
            description: None,
            stack: vec![],
        },
    )
    .unwrap();
    let err = cfg
        .add_repo(
            "bar".into(),
            Repo {
                path: PathBuf::from("/tmp/shared"),
                description: None,
                stack: vec![],
            },
        )
        .unwrap_err();

    assert_eq!(err.exit_code(), 5);
    assert!(
        !cfg.repos().contains_key("bar"),
        "conflicting name not added"
    );
}

#[test]
fn remove_nonexistent_returns_not_found() {
    let mut cfg = Config::default();
    let err = cfg.remove_repo("ghost").unwrap_err();
    assert_eq!(err.exit_code(), 3, "not-found maps to exit 3");
}
