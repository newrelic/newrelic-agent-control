//! Versioning for the agent-type *schema language* itself, decoupled from both the agent type
//! `version` (semver) and the Agent Control release version.
//!
//! Agent Control declares a single [SUPPORTED_PROTOCOL_VERSION] (the maximum it understands). Every
//! agent type file must declare a `protocol_version` (a quoted `MAJOR.MINOR` string), which is
//! validated against the supported version at the registry ingestion boundary before the rest of
//! the file is parsed.
//!
//! Compatibility rules for a file's version against the supported version:
//! - different `major` (either direction) → rejected: a major bump is a breaking schema change.
//! - same `major`, higher `minor` → rejected: the file is newer than this Agent Control understands.
//! - same `major`, equal or lower `minor` → accepted: minor bumps are additive and backward-compatible.
//!
//! For example, an Agent Control supporting `1.6` accepts `1.0`..=`1.6`, rejects `1.7` (too new),
//! and rejects `0.9` and `2.0` (wrong major).

use std::fmt::{self, Display};
use std::str::FromStr;
use thiserror::Error;

/// Maximum protocol version this Agent Control understands.
pub const SUPPORTED_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 0, minor: 1 };

#[derive(Error, Debug, PartialEq, Eq)]
pub enum ProtocolVersionError {
    #[error("missing required field protocol_version")]
    Missing,
    #[error("invalid protocol_version \"{0}\": expected a quoted MAJOR.MINOR string")]
    InvalidFormat(String),
    #[error(
        "unsupported protocol_version {target}: incompatible major version (this agent control supports {supported})"
    )]
    IncompatibleMajor {
        target: ProtocolVersion,
        supported: ProtocolVersion,
    },
    #[error(
        "unsupported protocol_version {target}: newer than supported (this agent control supports up to {supported})"
    )]
    TooNew {
        target: ProtocolVersion,
        supported: ProtocolVersion,
    },
}

/// A two-part `MAJOR.MINOR` schema-language version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolVersion {
    major: u64,
    minor: u64,
}

impl Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl FromStr for ProtocolVersion {
    type Err = ProtocolVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || ProtocolVersionError::InvalidFormat(s.to_string());
        let [major, minor] = s.split('.').collect::<Vec<_>>()[..] else {
            return Err(invalid());
        };
        Ok(ProtocolVersion {
            major: major.parse().map_err(|_| invalid())?,
            minor: minor.parse().map_err(|_| invalid())?,
        })
    }
}

impl ProtocolVersion {
    /// Checks this version against [SUPPORTED_PROTOCOL_VERSION].
    /// A different major is a breaking change in either direction; a higher minor under the same major
    /// is too new for this Agent Control. Equal or lower minors under the same major are additive and
    /// backward-compatible.
    pub fn is_supported(&self) -> Result<(), ProtocolVersionError> {
        check_compatibility(*self, SUPPORTED_PROTOCOL_VERSION)
    }
}

fn check_compatibility(
    target: ProtocolVersion,
    supported: ProtocolVersion,
) -> Result<(), ProtocolVersionError> {
    if target.major != supported.major {
        return Err(ProtocolVersionError::IncompatibleMajor { target, supported });
    }
    if target.minor > supported.minor {
        return Err(ProtocolVersionError::TooNew { target, supported });
    }
    Ok(())
}

/// Validates the `protocol_version` field of an already-parsed agent-type document. It reads only
/// that field and ignores the rest, so it can run before the document is converted into a
/// definition.
pub fn check(document: &serde_json::Value) -> Result<(), ProtocolVersionError> {
    let version = match document.get("protocol_version") {
        None => return Err(ProtocolVersionError::Missing),
        Some(serde_json::Value::String(raw)) => ProtocolVersion::from_str(raw)?,
        Some(other) => return Err(ProtocolVersionError::InvalidFormat(other.to_string())),
    };

    version.is_supported()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("0.1", ProtocolVersion { major: 0, minor: 1 })]
    #[case("1.0", ProtocolVersion { major: 1, minor: 0 })]
    #[case("12.34", ProtocolVersion { major: 12, minor: 34 })]
    fn parses_valid_versions(#[case] input: &str, #[case] expected: ProtocolVersion) {
        assert_eq!(ProtocolVersion::from_str(input).unwrap(), expected);
    }

    #[rstest]
    #[case::three_parts("1.2.0")]
    #[case::one_part("1")]
    #[case::non_numeric("x.y")]
    #[case::empty("")]
    #[case::trailing_dot("1.")]
    fn rejects_invalid_format(#[case] input: &str) {
        assert_eq!(
            ProtocolVersion::from_str(input),
            Err(ProtocolVersionError::InvalidFormat(input.to_string()))
        );
    }

    #[rstest]
    // Same major, lower/equal minor is accepted; higher minor is too new.
    #[case(pv(1, 5), pv(1, 6), Ok(()))]
    #[case(pv(1, 6), pv(1, 6), Ok(()))]
    #[case(pv(1, 0), pv(1, 6), Ok(()))]
    #[case(pv(1, 7), pv(1, 6), Err(ProtocolVersionError::TooNew { target: pv(1, 7), supported: pv(1, 6) }))]
    // Any major mismatch is rejected in either direction.
    #[case(pv(0, 9), pv(1, 6), Err(ProtocolVersionError::IncompatibleMajor { target: pv(0, 9), supported: pv(1, 6) }))]
    #[case(pv(2, 0), pv(1, 6), Err(ProtocolVersionError::IncompatibleMajor { target: pv(2, 0), supported: pv(1, 6) }))]
    fn compatibility_matrix(
        #[case] target: ProtocolVersion,
        #[case] supported: ProtocolVersion,
        #[case] expected: Result<(), ProtocolVersionError>,
    ) {
        assert_eq!(check_compatibility(target, supported), expected);
    }

    #[test]
    fn check_accepts_supported_version() {
        assert_eq!(
            check(&yaml_value("protocol_version: \"0.1\"\nname: whatever")),
            Ok(())
        );
    }

    #[test]
    fn check_reports_missing_field() {
        assert_eq!(
            check(&yaml_value("name: whatever")),
            Err(ProtocolVersionError::Missing)
        );
    }

    #[test]
    fn check_rejects_unquoted_float() {
        // An unquoted `0.1` is a yaml float, not the required quoted MAJOR.MINOR string.
        assert_eq!(
            check(&yaml_value("protocol_version: 0.1")),
            Err(ProtocolVersionError::InvalidFormat("0.1".to_string()))
        );
    }

    #[test]
    fn check_rejects_incompatible_version() {
        assert_eq!(
            check(&yaml_value("protocol_version: \"1.0\"")),
            Err(ProtocolVersionError::IncompatibleMajor {
                target: pv(1, 0),
                supported: SUPPORTED_PROTOCOL_VERSION,
            })
        );
    }

    fn pv(major: u64, minor: u64) -> ProtocolVersion {
        ProtocolVersion { major, minor }
    }

    fn yaml_value(content: &str) -> serde_json::Value {
        serde_saphyr::from_str(content).unwrap()
    }
}
