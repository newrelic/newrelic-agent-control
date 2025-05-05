use semver::Version;
use serde::{Deserialize, Deserializer, Serializer};
use std::fmt::{Display, Formatter};
use thiserror::Error;

const NAME_NAMESPACE_MIN_LENGTH: usize = 1;
const NAME_NAMESPACE_MAX_LENGTH: usize = 64;

#[derive(Error, Debug, PartialEq)]
pub enum AgentTypeIDError {
    #[error("Invalid AgentType namespace")]
    InvalidNamespace,
    #[error("Invalid AgentType name")]
    InvalidName,
    #[error("Invalid AgentType version")]
    InvalidVersion,
}

/// Holds agent type metadata that uniquely identifies an agent type.
/// Data can be represented as a fully qualified name in the format `<namespace>/<name>:<version>`.
#[derive(Debug, PartialEq, Clone)]
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

    fn has_valid_format(s: &str) -> bool {
        s.len() >= NAME_NAMESPACE_MIN_LENGTH
            && s.len() <= NAME_NAMESPACE_MAX_LENGTH
            && s.starts_with(|c: char| c.is_ascii_alphabetic())
            && s.ends_with(|c: char| c.is_ascii_alphanumeric())
            && s.chars().all(|c| {
                c.eq(&'-')
                    || c.eq(&'_')
                    || c.eq(&'.')
                    || c.is_ascii_digit()
                    || c.is_ascii_lowercase()
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
        if !AgentTypeID::has_valid_format(namespace.as_str()) {
            return Err(AgentTypeIDError::InvalidNamespace);
        }

        let name: String = value
            .chars()
            .skip_while(|&i| i != '/')
            .skip(1)
            .take_while(|&i| i != ':')
            .collect();
        if !AgentTypeID::has_valid_format(name.as_str()) {
            return Err(AgentTypeIDError::InvalidName);
        }

        let version_str: String = value.chars().skip_while(|&i| i != ':').skip(1).collect();

        let version =
            Version::parse(version_str.as_str()).map_err(|_| AgentTypeIDError::InvalidVersion)?;

        Ok(Self {
            name,
            namespace,
            version,
        })
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
            name: String,
            namespace: String,
            version: String,
        }

        let intermediate_spec = IntermediateAgentMetadata::deserialize(deserializer)?;

        if !Self::has_valid_format(intermediate_spec.name.as_str()) {
            return Err(Error::custom(AgentTypeIDError::InvalidName));
        }
        if !Self::has_valid_format(intermediate_spec.namespace.as_str()) {
            return Err(Error::custom(AgentTypeIDError::InvalidNamespace));
        }

        let version = Version::parse(intermediate_spec.version.as_str())
            .map_err(|_| Error::custom(AgentTypeIDError::InvalidVersion))?;

        Ok(AgentTypeID {
            name: intermediate_spec.name,
            namespace: intermediate_spec.namespace,
            version,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::*;

    #[test]
    fn test_correct_agent_type_metadata() {
        let actual = serde_yaml::from_str::<AgentTypeID>(
            r#"
name: nrdot_special-with-all.characters
namespace: newrelic_special-with-all.characters
version: 0.1.0-alpha.1
"#,
        )
        .unwrap();

        assert_eq!("nrdot_special-with-all.characters", actual.name);
        assert_eq!("newrelic_special-with-all.characters", actual.namespace);
        assert_eq!("0.1.0-alpha.1", actual.version.to_string());
    }

    #[test]
    fn test_invalid_agent_type_metadata() {
        struct TestCase {
            name: &'static str,
            metadata: &'static str,
            expected_error: AgentTypeIDError,
        }
        impl TestCase {
            fn run(self) {
                let actual_err =
                    serde_yaml::from_str::<AgentTypeID>(self.metadata).expect_err(self.name);

                assert!(
                    actual_err
                        .to_string()
                        .eq(self.expected_error.to_string().as_str()),
                    "TestCase: {} Expected error: {:?}, got: {:?}",
                    self.name,
                    self.expected_error,
                    actual_err
                );
            }
        }
        let test_cases = vec![
            TestCase {
                name: "empty name",
                expected_error: AgentTypeIDError::InvalidName,
                metadata: r#"
            name:
            namespace: newrelic
            version: 0.1.0
            "#,
            },
            TestCase {
                name: "empty namespace",
                expected_error: AgentTypeIDError::InvalidNamespace,
                metadata: r#"
            name: nrdot
            namespace:
            version: 0.1.0
            "#,
            },
            TestCase {
                name: "empty version",
                expected_error: AgentTypeIDError::InvalidVersion,
                metadata: r#"
            name: nrdot
            namespace: newrelic
            version:
            "#,
            },
            TestCase {
                name: "error wrong version 1",
                expected_error: AgentTypeIDError::InvalidVersion,
                metadata: r#"
            name: nrdot
            namespace: newrelic
            version: 0
            "#,
            },
            TestCase {
                name: "error wrong version 2",
                expected_error: AgentTypeIDError::InvalidVersion,
                metadata: r#"
            name: nrdot
            namespace: newrelic
            version: adsf
            "#,
            },
            TestCase {
                name: "invalid characters on name",
                expected_error: AgentTypeIDError::InvalidName,
                metadata: r#"
            name: invalid/slash
            namespace: newrelic
            version: 0.1.0
            "#,
            },
            TestCase {
                name: "invalid characters on namespace",
                expected_error: AgentTypeIDError::InvalidNamespace,
                metadata: r#"
            name: nrdot
            namespace: invalid/slash
            version: 0.1.0
            "#,
            },
            TestCase {
                name: "name exceeding allowed number of chars",
                expected_error: AgentTypeIDError::InvalidName,
                metadata: r#"
            name: test_test_test_test_test_test_test_test_test_test_test_test_test_test
            namespace: newrelic
            version: 0.1.0
            "#,
            },
            TestCase {
                name: "namespace exceeding allowed number of chars",
                expected_error: AgentTypeIDError::InvalidNamespace,
                metadata: r#"
            name: nrdot
            namespace: test_test_test_test_test_test_test_test_test_test_test_test_test_test
            version: 0.1.0
            "#,
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    #[test]
    fn try_from_fqn_str() {
        let agent_id = AgentTypeID::try_from("ns/aa:1.1.3").unwrap();
        assert_eq!(agent_id.name, "aa");
        assert_eq!(agent_id.namespace, "ns");
        assert_eq!(agent_id.version.to_string(), "1.1.3".to_string());

        assert_eq!(
            AgentTypeID::try_from("aa").unwrap_err(),
            AgentTypeIDError::InvalidName
        );

        assert_eq!(
            AgentTypeID::try_from("aa:1.1.3").unwrap_err(),
            AgentTypeIDError::InvalidNamespace
        );

        assert_eq!(
            AgentTypeID::try_from("ns/-").unwrap_err(),
            AgentTypeIDError::InvalidName
        );

        assert_eq!(
            AgentTypeID::try_from("ns/aa:").unwrap_err(),
            AgentTypeIDError::InvalidVersion
        );

        assert_eq!(
            AgentTypeID::try_from("ns/:1.1.3").unwrap_err(),
            AgentTypeIDError::InvalidName
        );

        assert_eq!(
            AgentTypeID::try_from("/:").unwrap_err(),
            AgentTypeIDError::InvalidNamespace
        );
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

        let foo: Foo = serde_yaml::from_str(fqn).unwrap();

        assert_eq!(foo.agent_type_id.name, "aa");
        assert_eq!(foo.agent_type_id.namespace, "ns");
        assert_eq!(foo.agent_type_id.version.to_string(), "1.0.0".to_string());

        assert_eq!(serde_yaml::to_string(&foo).unwrap(), fqn);

        let foo: Result<Foo, serde_yaml::Error> = serde_yaml::from_str(
            r#"
agent_type_id: namespace/name:invalid_version
"#,
        );
        assert!(
            foo.unwrap_err()
                .to_string()
                .contains("Invalid AgentType version")
        );
    }
}
