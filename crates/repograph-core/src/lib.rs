//! Core domain library for repograph.
//!
//! Owns the `Config` model, error type, and `git2` adapters. No clap, no
//! terminal I/O — the binary crate (and a future MCP server) depend on this
//! crate to keep their concerns separate.

pub mod config;
pub mod error;
pub mod git;

pub use config::{CONFIG_FILE_NAME, Config, Repo};
pub use error::RepographError;
pub use git::validate_git_repo;

/// Crate version, sourced from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
