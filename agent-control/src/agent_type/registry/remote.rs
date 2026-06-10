use super::{AgentTypeRegistry, AgentTypeRegistryError};
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::agent_type::definition::AgentTypeDefinition;
use crate::agent_type::oci::downloader::OCIAgentTypeDownloader;
use crate::environment::Environment;

/// An [AgentTypeRegistry] that resolves agent types by pulling them from a remote OCI registry
/// through an [OCIAgentTypeDownloader].
///
/// The agent type fully qualified name does not carry the platform/operating system; Agent Control
/// resolves them from its own running [Environment], which is used to build the OCI tag
/// `<platform>-<operating_system>-<name>-<version>` (the operating system segment is omitted on
/// kubernetes, matching how agent type metadata maps platform/os to [Environment]).
// TODO: not yet wired into the composite `Registry` (see its precedence TODO).
pub struct RemoteRegistry<D> {
    environment: Environment,
    downloader: D,
}

impl<D: OCIAgentTypeDownloader> RemoteRegistry<D> {
    pub fn new(environment: Environment, downloader: D) -> Self {
        Self {
            environment,
            downloader,
        }
    }

    /// Builds the OCI tag for the given id according to the running environment.
    fn artifact_tag(&self, agent_type_id: &AgentTypeID) -> String {
        let target = match self.environment {
            Environment::Linux => "host-linux",
            Environment::Windows => "host-windows",
            Environment::K8s => "kubernetes",
        };
        format!(
            "{target}-{}-{}",
            agent_type_id.name(),
            agent_type_id.version()
        )
    }
}

impl<D: OCIAgentTypeDownloader> AgentTypeRegistry for RemoteRegistry<D> {
    fn get(
        &self,
        agent_type_id: &AgentTypeID,
    ) -> Result<AgentTypeDefinition, AgentTypeRegistryError> {
        let tag = self.artifact_tag(agent_type_id);
        let raw = self
            .downloader
            .download(&tag)
            .map_err(|err| AgentTypeRegistryError::Remote(err.to_string()))?;

        let definition =
            AgentTypeDefinition::from_slice(&raw).map_err(AgentTypeRegistryError::Parsing)?;

        // The tag targets a single environment, so a definition for a different one means the
        // remote returned an artifact we did not ask for. Reject it rather than supervise an
        // agent type meant for another platform.
        let found = definition.metadata.environment;
        if found != self.environment {
            return Err(AgentTypeRegistryError::EnvironmentMismatch {
                requested: self.environment,
                found,
            });
        }

        Ok(definition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_type::oci::downloader::tests::{
        FakeDownloaderError, MockOCIAgentTypeDownloader,
    };
    use assert_matches::assert_matches;

    const K8S_DEFINITION: &str = r#"
namespace: newrelic
name: com.newrelic.infrastructure
version: 0.1.0
protocol_version: "1.0"
platform: kubernetes
deployment:
  objects: {}
"#;

    fn agent_type_id() -> AgentTypeID {
        AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.1.0").unwrap()
    }

    #[rstest::rstest]
    #[case::linux(Environment::Linux, "host-linux-com.newrelic.infrastructure-0.1.0")]
    #[case::windows(Environment::Windows, "host-windows-com.newrelic.infrastructure-0.1.0")]
    #[case::kubernetes(Environment::K8s, "kubernetes-com.newrelic.infrastructure-0.1.0")]
    fn test_artifact_tag(#[case] environment: Environment, #[case] expected_tag: &str) {
        let registry = RemoteRegistry::new(environment, MockOCIAgentTypeDownloader::new());
        assert_eq!(registry.artifact_tag(&agent_type_id()), expected_tag);
    }

    #[test]
    fn test_get_downloads_parses_and_returns_definition() {
        let mut downloader = MockOCIAgentTypeDownloader::new();
        downloader
            .expect_download()
            .withf(|tag| tag == "kubernetes-com.newrelic.infrastructure-0.1.0")
            .once()
            .returning(|_| Ok(K8S_DEFINITION.as_bytes().to_vec()));

        let registry = RemoteRegistry::new(Environment::K8s, downloader);
        let definition = registry.get(&agent_type_id()).unwrap();

        assert_eq!(definition.metadata.environment, Environment::K8s);
    }

    #[test]
    fn test_get_rejects_definition_for_a_different_environment() {
        let mut downloader = MockOCIAgentTypeDownloader::new();
        // The downloader returns a kubernetes definition while the registry runs on Linux.
        downloader
            .expect_download()
            .returning(|_| Ok(K8S_DEFINITION.as_bytes().to_vec()));

        let registry = RemoteRegistry::new(Environment::Linux, downloader);
        assert_matches!(
            registry.get(&agent_type_id()),
            Err(AgentTypeRegistryError::EnvironmentMismatch {
                requested: Environment::Linux,
                found: Environment::K8s,
            })
        );
    }

    #[test]
    fn test_get_maps_download_failure_to_remote_error() {
        let mut downloader = MockOCIAgentTypeDownloader::new();
        downloader
            .expect_download()
            .returning(|_| Err(FakeDownloaderError("boom".to_string())));

        let registry = RemoteRegistry::new(Environment::K8s, downloader);
        assert_matches!(
            registry.get(&agent_type_id()),
            Err(AgentTypeRegistryError::Remote(_))
        );
    }

    #[test]
    fn test_get_maps_invalid_definition_to_serialization_error() {
        let mut downloader = MockOCIAgentTypeDownloader::new();
        downloader
            .expect_download()
            .returning(|_| Ok(b"this is not a valid agent type".to_vec()));

        let registry = RemoteRegistry::new(Environment::K8s, downloader);
        assert_matches!(
            registry.get(&agent_type_id()),
            Err(AgentTypeRegistryError::Parsing(_))
        );
    }
}
