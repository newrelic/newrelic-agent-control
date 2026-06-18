mod local;
pub mod remote;

use std::path::PathBuf;

use thiserror::Error;
use tracing::{debug, warn};

use self::local::LocalRegistry;
use self::remote::RemoteRegistry;
use super::agent_type_id::AgentTypeID;
use super::definition::AgentTypeDefinition;
use crate::agent_type::definition::AgentTypeDefinitionParseError;
use crate::agent_type::oci::downloader::OCIAgentTypeArtifactDownloader;
use crate::environment::Environment;

#[derive(Error, Debug)]
pub enum AgentTypeRegistryError {
    #[error("agent type {0} not found")]
    NotFound(String),
    #[error("agent {0} already exists")]
    AlreadyExists(String),
    #[error("invalid agent type definition: {0}")]
    Parsing(AgentTypeDefinitionParseError),
    #[error("remote registry error: {0}")]
    Remote(String),
    #[error("metadata mismatch for '{tag}': {details}")]
    MetadataMismatch { tag: String, details: String },
}

/// Defines how to return an [AgentTypeDefinition] given an identifier.
pub trait AgentTypeRegistry {
    /// Returns an Agent type given its id.
    fn get(
        &self,
        agent_type_id: &AgentTypeID,
    ) -> Result<AgentTypeDefinition, AgentTypeRegistryError>;
}

/// Holds the information to initialize a [Registry].
pub struct RegistryConfig {
    /// Folder containing dynamic Agent Types, such Agent Types will take precedence over any other
    /// Agent Type definition.
    pub dynamic_agent_types_path: PathBuf,
}

/// The agent type registry used across Agent Control.
///
/// Resolves an [AgentTypeID] by walking an ordered list of inner registries: the first one that
/// returns a definition wins. Any error from a layer is recorded and the walk continues to the
/// next layer. If no layer succeeds, the composite returns the last error encountered.
///
/// `R` defaults to [SupportedRegistry] — the production composition (Local + Remote). The generic
/// is there so unit tests can plug in mock implementations of [AgentTypeRegistry] without an OCI
/// client.
pub struct Registry<R: AgentTypeRegistry = SupportedRegistry> {
    registries: Vec<R>,
}

impl<R: AgentTypeRegistry> Registry<R> {
    pub fn new(registries: Vec<R>) -> Self {
        Self { registries }
    }
}

#[allow(clippy::large_enum_variant)]
pub enum SupportedRegistry {
    Local(LocalRegistry),
    Remote(RemoteRegistry<OCIAgentTypeArtifactDownloader>),
}

impl AgentTypeRegistry for SupportedRegistry {
    fn get(
        &self,
        agent_type_id: &AgentTypeID,
    ) -> Result<AgentTypeDefinition, AgentTypeRegistryError> {
        match self {
            Self::Local(r) => r.get(agent_type_id),
            Self::Remote(r) => r.get(agent_type_id),
        }
    }
}

impl Registry<SupportedRegistry> {
    pub fn build(
        env: Environment,
        config: RegistryConfig,
        downloader: OCIAgentTypeArtifactDownloader,
    ) -> Self {
        let local = LocalRegistry::new(env, config.dynamic_agent_types_path);
        let remote = RemoteRegistry::new(env, downloader);
        Self::new(vec![
            SupportedRegistry::Local(local),
            SupportedRegistry::Remote(remote),
        ])
    }
}

impl<R: AgentTypeRegistry> AgentTypeRegistry for Registry<R> {
    fn get(
        &self,
        agent_type_id: &AgentTypeID,
    ) -> Result<AgentTypeDefinition, AgentTypeRegistryError> {
        let mut last_err = AgentTypeRegistryError::NotFound(agent_type_id.to_string());
        for (index, inner) in self.registries.iter().enumerate() {
            match inner.get(agent_type_id) {
                Ok(def) => {
                    debug!(
                        agent_type_id = %agent_type_id,
                        "Agent type definition found on registry layer \"{index}\"",
                    );
                    return Ok(def);
                }
                Err(err) => {
                    match err {
                        AgentTypeRegistryError::NotFound(_) => {
                            debug!(
                                agent_type_id = %agent_type_id,
                                error = %err,
                                "Agent type definition not found on registry layer \"{index}\"; falling through to the next layer",
                            );
                        }
                        _ => warn!(
                            agent_type_id = %agent_type_id,
                            error = %err,
                            "Agent type definition error on registry layer \"{index}\"; falling through to the next layer",
                        ),
                    }

                    last_err = err;
                }
            }
        }

        debug!(
            agent_type_id = %agent_type_id,
            error = %last_err,
            "Agent type definition not found on any registry layer",
        );

        Err(last_err)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use mockall::{mock, predicate};

    // Mock
    mock! {
        pub AgentTypeRegistry {}

        impl AgentTypeRegistry for AgentTypeRegistry  {
            fn get(&self, agent_type_id: &AgentTypeID) -> Result<AgentTypeDefinition, AgentTypeRegistryError>;
        }
    }

    impl MockAgentTypeRegistry {
        pub fn should_get(
            &mut self,
            agent_type_id: AgentTypeID,
            final_agent: &AgentTypeDefinition,
        ) {
            let final_agent = final_agent.clone();
            self.expect_get()
                .with(predicate::eq(agent_type_id))
                .once()
                .returning(move |_| Ok(final_agent.clone()));
        }

        pub fn expect_get_not_found(&mut self, agent_type_id: AgentTypeID) {
            let fqn = agent_type_id.to_string();
            self.expect_get()
                .with(predicate::eq(agent_type_id))
                .once()
                .returning(move |_| Err(AgentTypeRegistryError::NotFound(fqn.clone())));
        }

        pub fn expect_get_remote_error(&mut self, agent_type_id: AgentTypeID) {
            let fqn = agent_type_id.to_string();
            self.expect_get()
                .with(predicate::eq(agent_type_id))
                .once()
                .returning(move |_| Err(AgentTypeRegistryError::Remote(fqn.clone())));
        }
    }

    impl From<AgentTypeDefinition> for Registry<SupportedRegistry> {
        fn from(value: AgentTypeDefinition) -> Self {
            Registry::new(vec![SupportedRegistry::Local(LocalRegistry::from(value))])
        }
    }

    #[test]
    fn test_stop_on_first_layer_hit() {
        let id = AgentTypeID::try_from("ns/agent:0.0.0").unwrap();
        let definition = AgentTypeDefinition::empty_with_metadata(
            AgentTypeID::try_from("ns/agent:0.0.0").unwrap(),
        );

        let mut first = MockAgentTypeRegistry::new();
        first.should_get(id.clone(), &definition);

        let mut second = MockAgentTypeRegistry::new();
        second.expect_get().never();

        let registry = Registry::new(vec![first, second]);
        assert_eq!(registry.get(&id).unwrap(), definition);
    }

    #[test]
    fn test_error_falls_through_to_next_layer() {
        let id = AgentTypeID::try_from("ns/agent:0.0.0").unwrap();
        let definition = AgentTypeDefinition::empty_with_metadata(
            AgentTypeID::try_from("ns/agent:0.0.0").unwrap(),
        );

        let mut first = MockAgentTypeRegistry::new();
        first.expect_get_not_found(id.clone());

        let mut second = MockAgentTypeRegistry::new();
        second.should_get(id.clone(), &definition);

        let registry = Registry::new(vec![first, second]);
        assert_eq!(registry.get(&id).unwrap(), definition);
    }

    #[test]
    fn test_no_layer_hit() {
        let id = AgentTypeID::try_from("ns/missing:0.0.0").unwrap();

        let mut first = MockAgentTypeRegistry::new();
        first.expect_get_not_found(id.clone());

        let mut second = MockAgentTypeRegistry::new();
        second.expect_get_remote_error(id.clone());

        let registry = Registry::new(vec![first, second]);

        assert_matches!(registry.get(&id), Err(AgentTypeRegistryError::Remote(_)));
    }

    #[test]
    fn empty_composite_returns_not_found() {
        let registry: Registry<MockAgentTypeRegistry> = Registry::new(vec![]);
        let id = AgentTypeID::try_from("ns/agent:0.0.0").unwrap();

        assert_matches!(
            registry.get(&id),
            Err(AgentTypeRegistryError::NotFound(name)) if name == "ns/agent:0.0.0"
        );
    }
}
