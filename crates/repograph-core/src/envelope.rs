//! Agent-facing JSON envelopes.
//!
//! These types are the single definition of the `--json` payload shapes for
//! `list`, `status`, and `find`. Both consumers derive from them: the CLI
//! renderers in the binary's `output.rs`, and the MCP tool results in the
//! binary's `mcp` module. Keeping one definition is what makes "MCP output is
//! byte-identical to `--json` output" a property of the type system rather
//! than a pair of hand-synchronised structs.
//!
//! `context` and `doctor` need no envelope here — [`crate::Context`] and
//! [`crate::DoctorReport`] already serialise as their own top-level payloads.
//!
//! Every type borrows rather than owns, so the CLI path stays allocation-free
//! beyond the entry vector it already built.

use std::path::Path;

use serde::Serialize;

use crate::config::Repo;
use crate::git::RepoStatus;
use crate::search::{FIND_SCHEMA_VERSION, Hit};

/// One repository as it appears in a `list` payload.
///
/// `description` is emitted as `null` rather than skipped: agents parsing the
/// envelope get a stable key set regardless of which optional fields are set.
#[derive(Debug, Serialize)]
pub struct ListEntry<'a> {
    pub name: &'a str,
    pub path: &'a Path,
    pub description: Option<&'a str>,
    pub stack: &'a [String],
}

impl<'a> ListEntry<'a> {
    /// Build a list entry over a registered repo and the name it is keyed by.
    #[must_use]
    pub fn new(name: &'a str, repo: &'a Repo) -> Self {
        Self {
            name,
            path: &repo.path,
            description: repo.description.as_deref(),
            stack: &repo.stack,
        }
    }
}

/// `repograph list --json` → `{ "repos": [...] }`.
///
/// Carries no `schema_version`; see the `context`/`doctor` envelopes for the
/// versioned shapes. Agents are told to read the field defensively.
#[derive(Debug, Serialize)]
pub struct ListEnvelope<'a> {
    pub repos: &'a [ListEntry<'a>],
}

/// `repograph status --json` → `{ "repos": [...] }`.
///
/// Each entry is a [`RepoStatus`], whose `error` field is always present and
/// `null` on healthy rows.
#[derive(Debug, Serialize)]
pub struct StatusEnvelope<'a> {
    pub repos: &'a [RepoStatus],
}

/// `repograph find --json` → the versioned hit envelope.
#[derive(Debug, Serialize)]
pub struct FindEnvelope<'a> {
    pub schema_version: u32,
    pub query: &'a str,
    /// Whether semantic (embedding) retrieval actually contributed to the
    /// ranking. `false` for a lexical-only query or when semantic degraded.
    pub semantic_used: bool,
    /// Reason semantic retrieval was requested but unavailable (missing
    /// feature, no embeddings, no model), or `null` when not requested or fully
    /// satisfied. Mirrors the stderr notice so stdout-only consumers can detect
    /// a keyword-only fallback.
    pub degraded: Option<&'a str>,
    pub hits: &'a [Hit],
}

impl<'a> FindEnvelope<'a> {
    /// Build a find envelope, stamping the current schema version.
    #[must_use]
    pub const fn new(
        query: &'a str,
        hits: &'a [Hit],
        semantic_used: bool,
        degraded: Option<&'a str>,
    ) -> Self {
        Self {
            schema_version: FIND_SCHEMA_VERSION,
            query,
            semantic_used,
            degraded,
            hits,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use std::path::PathBuf;

    use super::*;

    fn repo(path: &str, description: Option<&str>, stack: &[&str]) -> Repo {
        Repo {
            path: PathBuf::from(path),
            description: description.map(ToString::to_string),
            stack: stack.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn list_envelope_keeps_description_key_when_absent() {
        let r = repo("/tmp/api", None, &[]);
        let entries = vec![ListEntry::new("api", &r)];
        let v: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&ListEnvelope { repos: &entries }).unwrap(),
        )
        .unwrap();
        let entry = &v["repos"][0];
        assert!(
            entry.get("description").is_some(),
            "description key must be present even when null, got: {entry}"
        );
        assert!(entry["description"].is_null());
        assert_eq!(entry["stack"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn list_envelope_round_trips_populated_fields() {
        let r = repo("/tmp/web", Some("frontend"), &["ts", "react"]);
        let entries = vec![ListEntry::new("web", &r)];
        let v: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&ListEnvelope { repos: &entries }).unwrap(),
        )
        .unwrap();
        assert_eq!(v["repos"][0]["name"], "web");
        assert_eq!(v["repos"][0]["description"], "frontend");
        assert_eq!(v["repos"][0]["stack"][1], "react");
    }

    #[test]
    fn find_envelope_stamps_current_schema_version() {
        let env = FindEnvelope::new("jwt rotation", &[], false, None);
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&env).unwrap()).unwrap();
        assert_eq!(v["schema_version"], FIND_SCHEMA_VERSION);
        assert_eq!(v["query"], "jwt rotation");
        assert_eq!(v["semantic_used"], false);
        assert!(v["degraded"].is_null());
        assert_eq!(v["hits"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn status_envelope_wraps_entries_under_repos() {
        let envelope = StatusEnvelope { repos: &[] };
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&envelope).unwrap()).unwrap();
        assert!(v["repos"].is_array());
    }
}
