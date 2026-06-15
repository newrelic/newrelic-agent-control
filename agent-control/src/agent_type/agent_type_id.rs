use semver::Version;
use serde::{Deserialize, Deserializer, Serializer};
use std::fmt::{Display, Formatter};
use thiserror::Error;

const NAME_NAMESPACE_MIN_LENGTH: usize = 1;
pub(crate) const NAME_NAMESPACE_MAX_LENGTH: usize = 64;
/// Bounds the version length so a version bump (e.g. `9999.9999.999` -> `9999.9999.1000`)
/// cannot overflow external constraints like the OCI tag built using this metadata.
pub(crate) const VERSION_MAX_LENGTH: usize = 14;

#[derive(Error, Debug, PartialEq)]
pub enum AgentTypeIDError {
    #[error("invalid namespace: {0}")]
    InvalidNamespace(NameFormatError),
    #[error("invalid name: {0}")]
    InvalidName(NameFormatError),
    #[error("invalid version: {0}")]
    InvalidVersion(String),
    #[error("only Major.Minor.Patch semver format is allowed")]
    ForbiddenSemVer,
    #[error("version must not be longer than {max} characters, but it is {length}")]
    VersionTooLong { length: usize, max: usize },
}

#[derive(Error, Debug, PartialEq)]
pub enum NameFormatError {
    #[error("must not be empty")]
    Empty,
    #[error("must be at most {max} characters, but it is {length}")]
    TooLong { length: usize, max: usize },
    #[error("must start with an ASCII letter")]
    InvalidStart,
    #[error("must end with a letter or a digit")]
    InvalidEnd,
    #[error(
        "contains invalid character '{0}', only lowercase letters, digits, '.' and '_' are allowed"
    )]
    InvalidCharacter(char),
}

/// Holds agent type metadata that uniquely identifies an agent type.
/// Data can be represented as a fully qualified name in the format `<namespace>/<name>:<version>`.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct AgentTypeID {
    name: String,
    namespace: String,
    version: Version,
}

impl AgentTypeID {
    pub fn namespace(&self) -> &str {
        &self.namespace
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn version(&self) -> &Version {
        &self.version
    }

    /// Parses a semver version, additionally rejecting anything that isn't a digit or `.`.
    /// This constraint relates to the OCI tag built from this metadata.
    fn parse_version(s: &str) -> Result<Version, AgentTypeIDError> {
        if !s.chars().all(|c| c.is_ascii_digit() || c.eq(&'.')) {
            return Err(AgentTypeIDError::ForbiddenSemVer);
        }
        if s.len() > VERSION_MAX_LENGTH {
            return Err(AgentTypeIDError::VersionTooLong {
                length: s.len(),
                max: VERSION_MAX_LENGTH,
            });
        }
        Version::parse(s).map_err(|e| AgentTypeIDError::InvalidVersion(e.to_string()))
    }

    /// Limits the characters allowed so this could be used as identifier for k8s resources and other
    /// places like OCI tags. Returns the specific [NameFormatError] on the first rule violated.
    fn validate_format(s: &str) -> Result<(), NameFormatError> {
        if s.len() < NAME_NAMESPACE_MIN_LENGTH {
            return Err(NameFormatError::Empty);
        }
        if s.len() > NAME_NAMESPACE_MAX_LENGTH {
            return Err(NameFormatError::TooLong {
                length: s.len(),
                max: NAME_NAMESPACE_MAX_LENGTH,
            });
        }
        if !s.starts_with(|c: char| c.is_ascii_alphabetic()) {
            return Err(NameFormatError::InvalidStart);
        }
        if !s.ends_with(|c: char| c.is_ascii_alphanumeric()) {
            return Err(NameFormatError::InvalidEnd);
        }
        if let Some(invalid) = s
            .chars()
            .find(|c| !(c.eq(&'_') || c.eq(&'.') || c.is_ascii_digit() || c.is_ascii_lowercase()))
        {
            return Err(NameFormatError::InvalidCharacter(invalid));
        }
        Ok(())
    }

    fn from_parts(
        name: String,
        namespace: String,
        version: &str,
    ) -> Result<Self, AgentTypeIDError> {
        Self::validate_format(namespace.as_str()).map_err(AgentTypeIDError::InvalidNamespace)?;
        Self::validate_format(name.as_str()).map_err(AgentTypeIDError::InvalidName)?;
        let version = Self::parse_version(version)?;
        Ok(Self {
            name,
            namespace,
            version,
        })
    }

    /// Deserializes an AgentTypeID from a fully qualified name string using the TryFrom<str> implementation.
    pub fn deserialize_fqn<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;

        AgentTypeID::try_from(s.as_ref()).map_err(serde::de::Error::custom)
    }

    /// Serializes an AgentTypeID to a fully qualified name string using the Display implementation.
    pub fn serialize_fqn<S>(agent_type_id: &AgentTypeID, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let fqn = agent_type_id.to_string();
        serializer.serialize_str(fqn.as_str())
    }
}

/// String representation of the AgentTypeID in the form of fully qualified name.
/// Example: `newrelic/nrdot:0.1.0`
impl Display for AgentTypeID {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}:{}", self.namespace, self.name, self.version)
    }
}

/// Converts from a fully quilified name to an AgentTypeID.
/// The fully qualified name must be in the format `<namespace>/<name>:<version>`.
/// Example: `newrelic/nrdot:0.1.0`
impl TryFrom<&str> for AgentTypeID {
    type Error = AgentTypeIDError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let namespace: String = value.chars().take_while(|&i| i != '/').collect();
        let name: String = value
            .chars()
            .skip_while(|&i| i != '/')
            .skip(1)
            .take_while(|&i| i != ':')
            .collect();
        let version: String = value.chars().skip_while(|&i| i != ':').skip(1).collect();

        Self::from_parts(name, namespace, version.as_str())
    }
}

impl<'de> Deserialize<'de> for AgentTypeID {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        // intermediate serialization type to validate `default` and `required` fields
        #[derive(Debug, Deserialize)]
        struct IntermediateAgentMetadata {
            name: Option<String>,
            namespace: Option<String>,
            version: Option<String>,
        }

        let IntermediateAgentMetadata {
            name,
            namespace,
            version,
        } = IntermediateAgentMetadata::deserialize(deserializer)?;

        Self::from_parts(
            name.unwrap_or_default(),
            namespace.unwrap_or_default(),
            version.unwrap_or_default().as_str(),
        )
        .map_err(Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde::Serialize;

    use super::*;

    #[test]
    fn test_correct_agent_type_metadata() {
        let actual = serde_saphyr::from_str::<AgentTypeID>(
            r#"
name: nrdot_special.with.all.characters
namespace: newrelic_special.with.all.characters
version: 0.1.0
"#,
        )
        .unwrap();

        assert_eq!("nrdot_special.with.all.characters", actual.name);
        assert_eq!("newrelic_special.with.all.characters", actual.namespace);
        assert_eq!("0.1.0", actual.version.to_string());
    }

    /// A name/namespace longer than the allowed maximum (69 > 64 characters).
    const TOO_LONG_NAME_FQN: &str =
        "ns/test_test_test_test_test_test_test_test_test_test_test_test_test_test:0.1.0";
    const TOO_LONG_NAMESPACE_FQN: &str =
        "test_test_test_test_test_test_test_test_test_test_test_test_test_test/nrdot:0.1.0";

    /// Matches the expected [AgentTypeIDError] variant. Lets each [rstest] case carry the variant it
    /// expects without comparing against the (semver-derived) error message.
    type ErrorMatcher = fn(AgentTypeIDError) -> bool;

    #[rstest]
    #[case::empty_name("ns/:0.1.0", (|e| matches!(e, AgentTypeIDError::InvalidName(NameFormatError::Empty))) as ErrorMatcher)]
    #[case::name_does_not_start_with_letter("ns/1nrdot:0.1.0", (|e| matches!(e, AgentTypeIDError::InvalidName(NameFormatError::InvalidStart))) as ErrorMatcher)]
    #[case::name_does_not_end_with_alphanumeric("ns/nrdot.:0.1.0", (|e| matches!(e, AgentTypeIDError::InvalidName(NameFormatError::InvalidEnd))) as ErrorMatcher)]
    #[case::name_with_invalid_char("ns/nr@dot:0.1.0", (|e| matches!(e, AgentTypeIDError::InvalidName(NameFormatError::InvalidCharacter('@')))) as ErrorMatcher)]
    #[case::name_with_dash("ns/nr-dot:0.1.0", (|e| matches!(e, AgentTypeIDError::InvalidName(NameFormatError::InvalidCharacter('-')))) as ErrorMatcher)]
    #[case::name_too_long(TOO_LONG_NAME_FQN, (|e| matches!(e, AgentTypeIDError::InvalidName(NameFormatError::TooLong { .. }))) as ErrorMatcher)]
    #[case::empty_namespace("/nrdot:0.1.0", (|e| matches!(e, AgentTypeIDError::InvalidNamespace(NameFormatError::Empty))) as ErrorMatcher)]
    #[case::namespace_with_invalid_char("n@s/nrdot:0.1.0", (|e| matches!(e, AgentTypeIDError::InvalidNamespace(NameFormatError::InvalidCharacter('@')))) as ErrorMatcher)]
    // Without a `/`, the whole input is taken as the namespace and `:` is not an allowed character.
    #[case::missing_name_separator("aa:1.1.3", (|e| matches!(e, AgentTypeIDError::InvalidNamespace(NameFormatError::InvalidCharacter(':')))) as ErrorMatcher)]
    #[case::namespace_too_long(TOO_LONG_NAMESPACE_FQN, (|e| matches!(e, AgentTypeIDError::InvalidNamespace(NameFormatError::TooLong { .. }))) as ErrorMatcher)]
    #[case::empty_version("ns/nrdot:", (|e| matches!(e, AgentTypeIDError::InvalidVersion(_))) as ErrorMatcher)]
    #[case::incomplete_version("ns/nrdot:0", (|e| matches!(e, AgentTypeIDError::InvalidVersion(_))) as ErrorMatcher)]
    #[case::non_numeric_version("ns/nrdot:adsf", (|e| matches!(e, AgentTypeIDError::ForbiddenSemVer)) as ErrorMatcher)]
    #[case::pre_release_version("ns/nrdot:0.1.0-alpha.1", (|e| matches!(e, AgentTypeIDError::ForbiddenSemVer)) as ErrorMatcher)]
    #[case::build_metadata_version("ns/nrdot:0.1.0+build", (|e| matches!(e, AgentTypeIDError::ForbiddenSemVer)) as ErrorMatcher)]
    #[case::version_too_long("ns/nrdot:1111.1111.11111", (|e| matches!(e, AgentTypeIDError::VersionTooLong { length: 15, max: 14 })) as ErrorMatcher)]
    fn try_from_invalid_fqn(#[case] fqn: &str, #[case] is_expected_error: ErrorMatcher) {
        let error = AgentTypeID::try_from(fqn).unwrap_err();
        let rendered = format!("{error:?}");
        assert!(
            is_expected_error(error),
            "unexpected error variant: {rendered}"
        );
    }

    #[test]
    fn try_from_valid_fqn() {
        let agent_id = AgentTypeID::try_from("ns/aa:1.1.3").unwrap();
        assert_eq!(agent_id.name, "aa");
        assert_eq!(agent_id.namespace, "ns");
        assert_eq!(agent_id.version.to_string(), "1.1.3".to_string());

        // A plain Major.Minor.Patch version at the maximum allowed length is accepted.
        assert!(AgentTypeID::try_from("ns/aa:1111.1111.1111").is_ok());
    }

    #[test]
    fn fqn_serialize_deserialize() {
        #[derive(Debug, Deserialize, Serialize)]
        struct Foo {
            #[serde(deserialize_with = "AgentTypeID::deserialize_fqn")]
            #[serde(serialize_with = "AgentTypeID::serialize_fqn")]
            agent_type_id: AgentTypeID,
        }

        let fqn = "agent_type_id: ns/aa:1.0.0\n";

        let foo: Foo = serde_saphyr::from_str(fqn).unwrap();

        assert_eq!(foo.agent_type_id.name, "aa");
        assert_eq!(foo.agent_type_id.namespace, "ns");
        assert_eq!(foo.agent_type_id.version.to_string(), "1.0.0".to_string());

        assert_eq!(serde_saphyr::to_string(&foo).unwrap(), fqn);

        let foo: Result<Foo, serde_saphyr::Error> = serde_saphyr::from_str(
            r#"
agent_type_id: namespace/name:invalid_version
"#,
        );
        assert!(
            foo.unwrap_err()
                .to_string()
                .contains("only Major.Minor.Patch semver format is allowed")
        );
    }
}
