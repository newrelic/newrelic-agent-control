use crate::environment::Environment;
use semver::Version;
use serde::{Deserialize, Deserializer, Serializer};
use std::fmt::{Display, Formatter};
use thiserror::Error;

const NAME_NAMESPACE_MIN_LENGTH: usize = 1;
const NAME_NAMESPACE_MAX_LENGTH: usize = 64;

#[derive(Error, Debug, PartialEq)]
pub enum AgentTypeIDError {
    #[error("invalid AgentType namespace")]
    InvalidNamespace,
    #[error("invalid AgentType name")]
    InvalidName,
    #[error("invalid AgentType version")]
    InvalidVersion,
    #[error("missing AgentType platform")]
    MissingPlatform,
    #[error("operating_system is required when platform is host")]
    MissingOperatingSystem,
    #[error("operating_system must not be set when platform is k8s")]
    UnexpectedOperatingSystem,
}

/// Holds agent type metadata that uniquely identifies an agent type.
///
/// To keep backward compatibility with existing local and remote configs (which reference
/// agent types only by their fully qualified name), platform and operating system were
/// intentionally **not** added to the FQN format `<namespace>/<name>:<version>`. Identity —
/// and therefore [Hash]/[PartialEq]/[Eq] — follows the FQN tuple `(namespace, name, version)`.
/// `platform` and `operating_system` are auxiliary metadata describing which definition file
/// the id was loaded from, and do not participate in equality. This way an id parsed from a
/// YAML definition matches one built from its FQN string when used as a key in a registry,
/// and existing FQN-based references keep working unchanged.
#[derive(Debug, Clone)]
pub struct AgentTypeID {
    name: String,
    namespace: String,
    version: Version,
    platform: Option<Platform>,
    operating_system: Option<OperatingSystem>,
}

impl PartialEq for AgentTypeID {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.namespace == other.namespace
            && self.version == other.version
    }
}

impl Eq for AgentTypeID {}

impl std::hash::Hash for AgentTypeID {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.namespace.hash(state);
        self.version.hash(state);
    }
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

/// Converts from a fully qualified name to an AgentTypeID.
/// The fully qualified name must be in the format `<namespace>/<name>:<version>`.
/// Example: `newrelic/nrdot:0.1.0`
///
/// FQN strings don't carry platform/OS info, so the resulting [AgentTypeID] has
/// `environment == None`.
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
            platform: None,
            operating_system: None,
        })
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Platform {
    Host,
    Kubernetes,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OperatingSystem {
    Linux,
    Windows,
}

impl TryFrom<&AgentTypeID> for Environment {
    type Error = AgentTypeIDError;

    fn try_from(id: &AgentTypeID) -> Result<Self, Self::Error> {
        match (id.platform, id.operating_system) {
            (Some(Platform::Host), Some(OperatingSystem::Linux)) => Ok(Environment::Linux),
            (Some(Platform::Host), Some(OperatingSystem::Windows)) => Ok(Environment::Windows),
            (Some(Platform::Kubernetes), None) => Ok(Environment::K8s),
            (Some(Platform::Host), None) => Err(AgentTypeIDError::MissingOperatingSystem),
            (Some(Platform::Kubernetes), Some(_)) => {
                Err(AgentTypeIDError::UnexpectedOperatingSystem)
            }
            (None, _) => Err(AgentTypeIDError::MissingPlatform),
        }
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
            platform: Option<Platform>,
            operating_system: Option<OperatingSystem>,
        }

        let IntermediateAgentMetadata {
            name,
            namespace,
            version,
            platform,
            operating_system,
        } = IntermediateAgentMetadata::deserialize(deserializer)?;

        let name = name.unwrap_or_default();
        if !Self::has_valid_format(name.as_str()) {
            return Err(Error::custom(AgentTypeIDError::InvalidName));
        }

        let namespace = namespace.unwrap_or_default();
        if !Self::has_valid_format(namespace.as_str()) {
            return Err(Error::custom(AgentTypeIDError::InvalidNamespace));
        }

        let version = Version::parse(version.unwrap_or_default().as_str())
            .map_err(|_| Error::custom(AgentTypeIDError::InvalidVersion))?;

        Ok(AgentTypeID {
            name,
            namespace,
            version,
            platform,
            operating_system,
        })
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
name: nrdot_special-with-all.characters
namespace: newrelic_special-with-all.characters
version: 0.1.0-alpha.1
platform: kubernetes
"#,
        )
        .unwrap();

        assert_eq!("nrdot_special-with-all.characters", actual.name);
        assert_eq!("newrelic_special-with-all.characters", actual.namespace);
        assert_eq!("0.1.0-alpha.1", actual.version.to_string());
        assert_eq!(Some(Platform::Kubernetes), actual.platform);
        assert_eq!(None, actual.operating_system);
        assert_eq!(Environment::K8s, Environment::try_from(&actual).unwrap());
    }

    #[rstest]
    #[case::empty_name(
        r#"
        name:
        namespace: newrelic
        version: 0.1.0
        "#,
        AgentTypeIDError::InvalidName
    )]
    #[case::empty_namespace(
        r#"
        name: nrdot
        namespace:
        version: 0.1.0
        "#,
        AgentTypeIDError::InvalidNamespace
    )]
    #[case::empty_version(
        r#"
        name: nrdot
        namespace: newrelic
        version:
        "#,
        AgentTypeIDError::InvalidVersion
    )]
    #[case::version_not_semver(
        r#"
        name: nrdot
        namespace: newrelic
        version: 0
        "#,
        AgentTypeIDError::InvalidVersion
    )]
    #[case::version_garbage(
        r#"
        name: nrdot
        namespace: newrelic
        version: adsf
        "#,
        AgentTypeIDError::InvalidVersion
    )]
    #[case::invalid_chars_in_name(
        r#"
        name: invalid/slash
        namespace: newrelic
        version: 0.1.0
        "#,
        AgentTypeIDError::InvalidName
    )]
    #[case::invalid_chars_in_namespace(
        r#"
        name: nrdot
        namespace: invalid/slash
        version: 0.1.0
        "#,
        AgentTypeIDError::InvalidNamespace
    )]
    #[case::name_too_long(
        r#"
        name: test_test_test_test_test_test_test_test_test_test_test_test_test_test
        namespace: newrelic
        version: 0.1.0
        "#,
        AgentTypeIDError::InvalidName
    )]
    #[case::namespace_too_long(
        r#"
        name: nrdot
        namespace: test_test_test_test_test_test_test_test_test_test_test_test_test_test
        version: 0.1.0
        "#,
        AgentTypeIDError::InvalidNamespace
    )]
    fn test_invalid_agent_type_metadata(
        #[case] metadata: &str,
        #[case] expected_error: AgentTypeIDError,
    ) {
        let actual_err = serde_saphyr::from_str::<AgentTypeID>(metadata).unwrap_err();

        assert_eq!(actual_err.to_string(), expected_error.to_string());
    }

    #[test]
    fn try_from_fqn_str() {
        let agent_id = AgentTypeID::try_from("ns/aa:1.1.3").unwrap();
        assert_eq!(agent_id.name, "aa");
        assert_eq!(agent_id.namespace, "ns");
        assert_eq!(agent_id.version.to_string(), "1.1.3".to_string());
        assert_eq!(None, agent_id.platform);
        assert_eq!(None, agent_id.operating_system);
        assert_eq!(
            AgentTypeIDError::MissingPlatform,
            Environment::try_from(&agent_id).unwrap_err()
        );
    }

    #[rstest]
    #[case::missing_namespace_and_version("aa", AgentTypeIDError::InvalidName)]
    #[case::missing_namespace("aa:1.1.3", AgentTypeIDError::InvalidNamespace)]
    #[case::invalid_name_char("ns/-", AgentTypeIDError::InvalidName)]
    #[case::missing_version("ns/aa:", AgentTypeIDError::InvalidVersion)]
    #[case::empty_name("ns/:1.1.3", AgentTypeIDError::InvalidName)]
    #[case::all_empty("/:", AgentTypeIDError::InvalidNamespace)]
    fn try_from_fqn_str_invalid(#[case] input: &str, #[case] expected_error: AgentTypeIDError) {
        assert_eq!(AgentTypeID::try_from(input).unwrap_err(), expected_error);
    }

    #[rstest]
    #[case::host_linux(
        Some(Platform::Host),
        Some(OperatingSystem::Linux),
        Ok(Environment::Linux)
    )]
    #[case::host_windows(
        Some(Platform::Host),
        Some(OperatingSystem::Windows),
        Ok(Environment::Windows)
    )]
    #[case::kubernetes(Some(Platform::Kubernetes), None, Ok(Environment::K8s))]
    #[case::host_without_os(
        Some(Platform::Host),
        None,
        Err(AgentTypeIDError::MissingOperatingSystem)
    )]
    #[case::kubernetes_with_os(
        Some(Platform::Kubernetes),
        Some(OperatingSystem::Linux),
        Err(AgentTypeIDError::UnexpectedOperatingSystem)
    )]
    #[case::missing_platform(None, None, Err(AgentTypeIDError::MissingPlatform))]
    fn environment_try_from_agent_type_id(
        #[case] platform: Option<Platform>,
        #[case] operating_system: Option<OperatingSystem>,
        #[case] expected: Result<Environment, AgentTypeIDError>,
    ) {
        let id = AgentTypeID {
            name: "n".to_string(),
            namespace: "ns".to_string(),
            version: Version::parse("0.0.1").unwrap(),
            platform,
            operating_system,
        };

        assert_eq!(Environment::try_from(&id), expected);
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
                .contains("invalid AgentType version")
        );
    }
}
