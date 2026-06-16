//! Semantic-retrieval acceptance tests, gated behind the `semantic` feature.
//!
//! These exercise the real embedding runtime end-to-end: the first `index
//! --semantic` downloads the `bge-small-en-v1.5` ONNX model (~127 MB) into the
//! test's throwaway data dir, embeds the planted chunks, and the subsequent
//! `find --semantic` ranks by cosine over those vectors.
//!
//! They are `#[ignore]` by default — the model download needs network and is far
//! too heavy for ordinary CI. Run them deliberately with:
//!
//! ```sh
//! cargo test -p repograph --features semantic -- --ignored
//! ```
//!
//! The whole file compiles to nothing without the feature, so a default build is
//! unaffected.
#![cfg(feature = "semantic")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use predicates::prelude::*;

use crate::common::{fixture_git_repo_with_files, repograph_cmd};

fn register(config_dir: &Path, repo: &Path, name: &str) {
    repograph_cmd(config_dir)
        .arg("add")
        .arg(repo)
        .arg("--name")
        .arg(name)
        .assert()
        .success();
}

/// `index --semantic` must download the model, embed every chunk, and report
/// that embeddings were written — proving the runtime path actually executes.
#[test]
#[ignore = "downloads ~127MB embedding model; run with --features semantic -- --ignored"]
fn index_semantic_writes_embeddings_and_reports_it() {
    let tmp = tempfile::TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo_with_files(
        tmp.path(),
        "svc",
        &[("retry.rs", "pub fn retry_with_backoff() {}\n")],
    );
    register(&config_dir, &repo, "svc");

    repograph_cmd(&config_dir)
        .arg("index")
        .arg("--semantic")
        .assert()
        .success()
        // The summary names the embedding work on stderr; no `note:` degrade.
        .stderr(predicates::str::contains("with embeddings"))
        .stderr(predicates::str::contains("note:").not());
}

/// A paraphrased query must surface the conceptually-matching file that keyword
/// search misses. Verified against the same corpus: a lexical-only `find` of this
/// query returns only `color.rs` (an incidental stopword match), so `resilience.rs`
/// appearing under `--semantic` can only come from the embedding/vector path —
/// proof that semantic retrieval contributed, alongside the JSON envelope flags.
#[test]
#[ignore = "downloads ~127MB embedding model; run with --features semantic -- --ignored"]
fn find_semantic_ranks_by_meaning_and_marks_json_envelope() {
    let tmp = tempfile::TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo_with_files(
        tmp.path(),
        "svc",
        &[
            (
                "resilience.rs",
                "/// Retry an operation with exponential backoff between attempts.\n\
                 pub fn retry_with_backoff() {}\n",
            ),
            (
                "color.rs",
                "/// Convert an RGB triple to a hexadecimal color string.\n\
                 pub fn rgb_to_hex() {}\n",
            ),
        ],
    );
    register(&config_dir, &repo, "svc");
    repograph_cmd(&config_dir)
        .arg("index")
        .arg("--semantic")
        .assert()
        .success();

    let out = repograph_cmd(&config_dir)
        .arg("find")
        .arg("pause and try the request again after a transient failure")
        .arg("--semantic")
        .arg("--json")
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();

    assert_eq!(v["schema_version"], 2);
    assert_eq!(
        v["semantic_used"], true,
        "semantic retrieval contributed to the ranking"
    );
    assert!(
        v["degraded"].is_null(),
        "model present and embeddings written — nothing degraded"
    );
    let paths: Vec<&str> = v["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["path"].as_str().unwrap())
        .collect();
    assert!(
        paths.contains(&"resilience.rs"),
        "semantic retrieval surfaces the meaning-matched file BM25 alone misses: {paths:?}"
    );
}
