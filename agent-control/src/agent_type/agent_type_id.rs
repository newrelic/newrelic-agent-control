use semver::Version;
use serde::{Deserialize, Deserializer};
use std::fmt::{Display, Formatter};
use thiserror::Error;

const NAME_NAMESPACE_MIN_LENGTH: usize = 1;
const NAME_NAMESPACE_MAX_LENGTH: usize = 64;

#[derive(Error, Debug)]
pub enum AgentTypeIDError {
    #[error("AgentType must have a valid namespace")]
    InvalidNamespace,
    #[error("AgentType must have a valid name")]
    InvalidName,
    #[error("AgentType must have a valid version")]
    InvalidVersion,
}

/// Holds agent type metadata that uniquely identifies an agent type.
#[derive(Debug, PartialEq, Clone)]
pub struct AgentTypeID {
    pub name: String,
    pub namespace: String,
    pub version: Version,
}

impl AgentTypeID {
    fn check_string(s: &str) -> bool {
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
}

impl Display for AgentTypeID {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}:{}", self.namespace, self.name, self.version)
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
            version: Version,
        }

        // TODO add a explicit validation for version and use InvalidVersion error
        let intermediate_spec = IntermediateAgentMetadata::deserialize(deserializer)?;

        if !Self::check_string(intermediate_spec.name.as_str()) {
            return Err(Error::custom(AgentTypeIDError::InvalidName));
        }
        if !Self::check_string(intermediate_spec.namespace.as_str()) {
            return Err(Error::custom(AgentTypeIDError::InvalidNamespace));
        }

        Ok(AgentTypeID {
            name: intermediate_spec.name,
            namespace: intermediate_spec.namespace,
            version: intermediate_spec.version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE_CORRECT_METADATA: &str = r#"
name: nrdot_special-with-all.characters
namespace: newrelic_special-with-all.characters
version: 0.1.0-alpha.1
"#;
    #[test]
    fn test_correct_agent_type_metadata() {
        let actual = serde_yaml::from_str::<AgentTypeID>(EXAMPLE_CORRECT_METADATA).unwrap();

        assert_eq!("nrdot_special-with-all.characters", actual.name);
        assert_eq!("newrelic_special-with-all.characters", actual.namespace);
        assert_eq!("0.1.0-alpha.1", actual.version.to_string());
    }

    #[test]
    fn test_invalid_agent_type_metadata() {
        struct TestCase {
            name: &'static str,
            metadata: &'static str,
        }
        impl TestCase {
            fn run(self) {
                let actual = serde_yaml::from_str::<AgentTypeID>(self.metadata);

                assert!(actual.is_err(), "{}", self.name)
            }
        }
        let test_cases = vec![
            TestCase {
                name: "error no name",
                metadata: r#"
name:
namespace: newrelic
version: 0.1.0
"#,
            },
            TestCase {
                name: "error no namespace",
                metadata: r#"
name: nrdot
namespace:
version: 0.1.0
"#,
            },
            TestCase {
                name: "error no version",
                metadata: r#"
name: nrdot
namespace: newrelic
version:
"#,
            },
            TestCase {
                name: "error wrong version 1",
                metadata: r#"
name: nrdot
namespace: newrelic
version: 0
"#,
            },
            TestCase {
                name: "error wrong version 2",
                metadata: r#"
name: nrdot
namespace: newrelic
version: adsf
"#,
            },
            TestCase {
                name: "error wrong values on name",
                metadata: r#"
name: nrdot:
namespace: newrelic
version: 0.1.0
"#,
            },
            TestCase {
                name: "error wrong values on namespace",
                metadata: r#"
name: nrdot
namespace: newrelic/
version: 0.1.0
"#,
            },
            TestCase {
                name: "error name exceeding allowed number of chars",
                metadata: r#"
name: test_test_test_test_test_test_test_test_test_test_test_test_test_test
namespace: newrelic/
version: 0.1.0
"#,
            },
            TestCase {
                name: "error namespace exceeding allowed number of chars",
                metadata: r#"
name: nrdot
namespace: test_test_test_test_test_test_test_test_test_test_test_test_test_test/
version: 0.1.0
"#,
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }
}
