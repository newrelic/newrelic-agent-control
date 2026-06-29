//! OCI tagging for agent type artifacts and the downloader used to pull them from a remote
//! registry.
pub mod downloader;

use crate::agent_type::agent_type_id::AgentTypeID;
use crate::environment::Environment;
use std::fmt::{Display, Formatter};

/// The OCI image tag that identifies an agent type artifact in a remote registry.
///
/// Composed as `<platform>-<operating_system>-<name>-<version>`, omitting the operating system for
/// environments that don't have one (Kubernetes). The result is a valid OCI tag by construction:
/// the prefix is a fixed lowercase string, [AgentTypeID] constrains the name to `[a-z0-9._]` and
/// the version to `[0-9.]`, and the bounded name/version lengths keep it within the OCI length
/// limit (see the `longest_tag_never_overflows` test).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTypeTag(String);

impl AgentTypeTag {
    /// Builds the tag used to pull `agent_type_id` for the given running `environment`.
    pub fn new(agent_type_id: &AgentTypeID, environment: Environment) -> Self {
        Self(format!(
            "{}-{}-{}",
            environment_prefix(environment),
            agent_type_id.name(),
            agent_type_id.version()
        ))
    }

    /// Returns the tag as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for AgentTypeTag {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// The fixed `<platform>-<operating_system>` prefix for an environment.
fn environment_prefix(environment: Environment) -> &'static str {
    match environment {
        Environment::Linux => "host-linux",
        Environment::Windows => "host-windows",
        Environment::K8s => "kubernetes",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_type::agent_type_id::{NAME_NAMESPACE_MAX_LENGTH, VERSION_MAX_LENGTH};

    /// Maximum length of an OCI image tag (see the OCI distribution spec).
    const MAX_TAG_LENGTH: usize = 128;
    /// Amount of separator '-' without counting the one inside Environment
    const SEPARATORS_COUNT: usize = 2;

    #[rstest::rstest]
    #[case::linux(Environment::Linux, "host-linux-com.fake_name-0.1.0")]
    #[case::windows(Environment::Windows, "host-windows-com.fake_name-0.1.0")]
    #[case::kubernetes(Environment::K8s, "kubernetes-com.fake_name-0.1.0")]
    fn builds_tag_per_environment(#[case] environment: Environment, #[case] expected_tag: &str) {
        let agent_type_id = AgentTypeID::try_from("fake_ns/com.fake_name:0.1.0").unwrap();

        assert_eq!(
            AgentTypeTag::new(&agent_type_id, environment).as_str(),
            expected_tag
        );
    }

    /// Guards the only way the by-construction validity of [AgentTypeTag] can break: adding an
    /// [Environment] whose prefix is long enough that the longest possible tag
    /// `<prefix>-<name>-<version>` overflows [MAX_TAG_LENGTH]. The exhaustive match fails to
    /// compile when a variant is added, forcing a re-check here.
    #[test]
    fn longest_tag_never_overflows() {
        for environment in Environment::all() {
            let longest_tag_length = environment_prefix(environment).len()
                + SEPARATORS_COUNT
                + NAME_NAMESPACE_MAX_LENGTH
                + VERSION_MAX_LENGTH;
            assert!(
                longest_tag_length <= MAX_TAG_LENGTH,
                "environment {environment} can overflow the tag length: {longest_tag_length} > {MAX_TAG_LENGTH}"
            );
        }
    }
}
