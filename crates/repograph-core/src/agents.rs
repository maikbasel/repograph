//! Built-in registry of agent toolchains.
//!
//! Maps a small, hardcoded set of agent identifiers to the file patterns each
//! agent uses to store its rules inside a repository. The set is intentionally
//! not user-extensible — the contract is between `repograph` and the agent
//! toolchain ecosystem, not between `repograph` and each user's preferences.
//!
//! Adding a new agent is a one-line enum extension plus its `file_patterns`
//! arm. Removing one requires a deprecation period: one minor release where
//! the ID is accepted with a `warn!` and routed to a no-op pattern set.

use serde::{Deserialize, Serialize};

use crate::error::RepographError;

/// One of the agent toolchains repograph knows how to find rules for.
///
/// Serialized as a kebab-case string in TOML (`claude-code`, `agents-md`, …)
/// and JSON. Unknown IDs deserialize as a typed error via serde's default
/// rejection of unknown enum variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentId {
    /// Anthropic's Claude Code — `CLAUDE.md` at repo root.
    ClaudeCode,
    /// Cross-vendor `AGENTS.md` convention.
    AgentsMd,
    /// Cursor — `.cursor/rules/*.md` and legacy `.cursorrules`.
    Cursor,
    /// Aider — `CONVENTIONS.md`.
    Aider,
    /// Windsurf — `.windsurfrules`.
    Windsurf,
    /// GitHub Copilot — `.github/copilot-instructions.md`.
    Copilot,
}

impl AgentId {
    /// All known agent IDs in the v1 registry, in display order.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::ClaudeCode,
            Self::AgentsMd,
            Self::Cursor,
            Self::Aider,
            Self::Windsurf,
            Self::Copilot,
        ]
    }

    /// The kebab-case identifier used in TOML / JSON / CLI flags.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::AgentsMd => "agents-md",
            Self::Cursor => "cursor",
            Self::Aider => "aider",
            Self::Windsurf => "windsurf",
            Self::Copilot => "copilot",
        }
    }

    /// A short human-readable label for UI rendering (cliclack option labels).
    #[must_use]
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::AgentsMd => "AGENTS.md",
            Self::Cursor => "Cursor",
            Self::Aider => "Aider",
            Self::Windsurf => "Windsurf",
            Self::Copilot => "GitHub Copilot",
        }
    }

    /// The glob-style file patterns this agent stores its rules at, relative
    /// to a repository's root. Returned slice is always non-empty.
    #[must_use]
    pub const fn file_patterns(&self) -> &'static [&'static str] {
        match self {
            Self::ClaudeCode => &["CLAUDE.md"],
            Self::AgentsMd => &["AGENTS.md"],
            Self::Cursor => &[".cursor/rules/*.md", ".cursorrules"],
            Self::Aider => &["CONVENTIONS.md"],
            Self::Windsurf => &[".windsurfrules"],
            Self::Copilot => &[".github/copilot-instructions.md"],
        }
    }

    /// Parse a kebab-case agent ID string. Used by the `--agents` CLI flag.
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::InvalidName`] with `kind = "agent"` when the
    /// input is not one of the v1 registry entries. The error maps to exit
    /// code `2` (usage error) per the documented contract.
    pub fn parse(s: &str) -> Result<Self, RepographError> {
        for id in Self::all() {
            if id.as_str() == s {
                return Ok(*id);
            }
        }
        Err(RepographError::InvalidName {
            kind: "agent",
            name: s.to_string(),
            reason: "not a recognized agent ID",
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn all_contains_every_variant_exactly_once() {
        let all = AgentId::all();
        assert_eq!(all.len(), 6, "v1 registry has six entries");
        let mut seen = std::collections::BTreeSet::new();
        for id in all {
            assert!(seen.insert(*id), "duplicate variant in all()");
        }
    }

    #[test]
    fn file_patterns_match_spec_table() {
        assert_eq!(AgentId::ClaudeCode.file_patterns(), &["CLAUDE.md"]);
        assert_eq!(AgentId::AgentsMd.file_patterns(), &["AGENTS.md"]);
        assert_eq!(
            AgentId::Cursor.file_patterns(),
            &[".cursor/rules/*.md", ".cursorrules"]
        );
        assert_eq!(AgentId::Aider.file_patterns(), &["CONVENTIONS.md"]);
        assert_eq!(AgentId::Windsurf.file_patterns(), &[".windsurfrules"]);
        assert_eq!(
            AgentId::Copilot.file_patterns(),
            &[".github/copilot-instructions.md"]
        );
    }

    #[test]
    fn file_patterns_are_non_empty_for_every_id() {
        for id in AgentId::all() {
            assert!(
                !id.file_patterns().is_empty(),
                "{id:?} has empty file_patterns"
            );
        }
    }

    #[test]
    fn parse_accepts_kebab_case_ids() {
        assert_eq!(AgentId::parse("claude-code").unwrap(), AgentId::ClaudeCode);
        assert_eq!(AgentId::parse("agents-md").unwrap(), AgentId::AgentsMd);
        assert_eq!(AgentId::parse("cursor").unwrap(), AgentId::Cursor);
        assert_eq!(AgentId::parse("aider").unwrap(), AgentId::Aider);
        assert_eq!(AgentId::parse("windsurf").unwrap(), AgentId::Windsurf);
        assert_eq!(AgentId::parse("copilot").unwrap(), AgentId::Copilot);
    }

    #[test]
    fn parse_rejects_unknown_id_with_invalid_name_kind_agent() {
        let err = AgentId::parse("bogus").unwrap_err();
        match err {
            RepographError::InvalidName { kind, name, .. } => {
                assert_eq!(kind, "agent");
                assert_eq!(name, "bogus");
            }
            other => panic!("expected InvalidName, got {other:?}"),
        }
    }

    #[test]
    fn parse_rejects_pascal_case() {
        // Sanity: we only accept the on-the-wire kebab form.
        assert!(AgentId::parse("ClaudeCode").is_err());
    }

    #[test]
    fn parse_error_exit_code_is_2() {
        let err = AgentId::parse("bogus").unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn serde_round_trip_through_toml_value() {
        let original = vec![AgentId::ClaudeCode, AgentId::Cursor, AgentId::AgentsMd];
        let serialized = toml::to_string(&toml::Table::from_iter([(
            "selected".to_string(),
            toml::Value::try_from(&original).unwrap(),
        )]))
        .unwrap();
        assert!(
            serialized.contains("\"claude-code\""),
            "kebab-case form on the wire, got: {serialized}"
        );
        assert!(serialized.contains("\"cursor\""));
        assert!(serialized.contains("\"agents-md\""));

        #[derive(Deserialize)]
        struct Wrap {
            selected: Vec<AgentId>,
        }
        let parsed: Wrap = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.selected, original);
    }

    #[test]
    fn serde_rejects_unknown_id() {
        #[derive(Debug, Deserialize)]
        struct Wrap {
            #[allow(dead_code)]
            selected: Vec<AgentId>,
        }
        let err = toml::from_str::<Wrap>("selected = [\"claude-code\", \"bogus\"]").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("bogus") || msg.contains("unknown variant"),
            "unknown variant should be named, got: {msg}"
        );
    }

    #[test]
    fn as_str_round_trips_through_parse() {
        for id in AgentId::all() {
            assert_eq!(AgentId::parse(id.as_str()).unwrap(), *id);
        }
    }
}
