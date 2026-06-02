//! Core domain library for repograph.
//!
//! Owns the `Config` model, error type, and `git2` adapters. No clap, no
//! terminal I/O — the binary crate (and a future MCP server) depend on this
//! crate to keep their concerns separate.

pub mod agent_artifact;
pub mod agents;
pub mod config;
pub mod context;
pub mod doctor;
pub mod error;
pub mod git;
pub mod path;

pub use agent_artifact::{
    ArtifactResult, BODY as AGENT_ARTIFACT_BODY, DELIMITER_BEGIN, DELIMITER_END,
    REASON_COPILOT_DEFERRED, SUMMARY as AGENT_ARTIFACT_SUMMARY, has_artifact_writer,
    install_artifacts, install_one, render_artifact, resolve_path, scope_is_meaningful,
};
pub use agents::AgentId;
pub use config::{
    Agents, CONFIG_FILE_NAME, Config, MAX_WORKSPACE_NAME_LEN, RESERVED_WORKSPACE_NAMES, Repo,
    Settings, Workspace, WorkspaceResolution, validate_workspace_name,
};
pub use context::{
    AgentDoc, Context, MatchedFile, RepoContext, SCHEMA_VERSION, Scope, resolve_agent_docs,
};
pub use doctor::{Check, DOCTOR_SCHEMA_VERSION, DoctorReport, Finding, Severity, Summary};
pub use error::RepographError;
pub use git::{RepoState, RepoStatus, inspect, validate_git_repo};

/// Crate version, sourced from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
