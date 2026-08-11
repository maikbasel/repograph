//! RFC 3339 timestamps for agent-facing envelopes.
//!
//! `context`, `doctor`, and the MCP tools that wrap them all stamp
//! `generated_at`. One definition keeps the format identical across every
//! payload, which matters because envelope parity between the CLI and the MCP
//! surface is an asserted property, not a convention.

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Fallback stamp used when formatting the current time fails.
///
/// Formatting `OffsetDateTime::now_utc()` as RFC 3339 has no realistic failure
/// mode, but the contract says `generated_at` is always present and always
/// parseable — so the error path yields a valid timestamp rather than an
/// `unwrap` or an absent field.
const EPOCH_RFC3339: &str = "1970-01-01T00:00:00Z";

/// Current UTC time formatted as RFC 3339.
pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| EPOCH_RFC3339.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Parsing back would need `time`'s `parsing` feature, which the binary does
    // not otherwise carry; assert the shape instead of round-tripping.
    #[test]
    fn produces_an_rfc3339_shaped_stamp() {
        let s = now_rfc3339();
        let bytes = s.as_bytes();
        assert!(s.len() >= 20, "too short for RFC 3339: {s}");
        assert_eq!(bytes[4], b'-', "expected YYYY-MM-DD, got: {s}");
        assert_eq!(bytes[7], b'-', "expected YYYY-MM-DD, got: {s}");
        assert_eq!(bytes[10], b'T', "expected date/time separator, got: {s}");
        assert_eq!(bytes[13], b':', "expected HH:MM:SS, got: {s}");
        assert_eq!(bytes[16], b':', "expected HH:MM:SS, got: {s}");
    }

    #[test]
    fn stamp_is_utc() {
        assert!(now_rfc3339().ends_with('Z'), "expected a UTC stamp");
    }
}
