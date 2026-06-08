mod local;

use std::path::PathBuf;

use thiserror::Error;

use self::local::LocalRegistry;
use super::agent_type_id::AgentTypeID;
use super::definition::AgentTypeDefinition;
use crate::environment::Environment;

#[derive(Error, Debug)]
pub enum AgentTypeRegistryError {
    #[error("agent type {0} not found")]
    NotFound(String),
    #[error("agent {0} already exists")]
    AlreadyExists(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_saphyr::Error),
    #[error("value conversion error: {0}")]
    ValueConversion(#[from] serde_json::Error),
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
/// It resolves agent type definitions delegating to the internal implementations.
#[derive(Debug)]
pub struct Registry {
    local: LocalRegistry,
    // TODO: add support for Remote Registry and apply the precedence rules as needed.
    // As `LocalRegistry` implements AgentTypeRegistry a `Vec<impl AgentTypeRegistry>` could be considered.
}

impl Registry {
    /// Builds a [Registry] whose local source loads the embedded agent types matching the given
    /// [Environment] and overlays the dynamic agent types found in the given directory (dynamic
    /// definitions take precedence).
    pub fn new(env: Environment, config: RegistryConfig) -> Self {
        Self {
            local: LocalRegistry::new(env, config.dynamic_agent_types_path),
        }
    }
}

impl AgentTypeRegistry for Registry {
    fn get(
        &self,
        agent_type_id: &AgentTypeID,
    ) -> Result<AgentTypeDefinition, AgentTypeRegistryError> {
        self.local.get(agent_type_id)
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
    }

    impl From<AgentTypeDefinition> for Registry {
        fn from(value: AgentTypeDefinition) -> Self {
            Self {
                local: LocalRegistry::from(value),
            }
        }
    }

    #[test]
    fn get_returns_the_definition_when_present() {
        let id = AgentTypeID::try_from("ns/agent:0.0.0").unwrap();
        let definition = AgentTypeDefinition::empty_with_metadata(id.clone());

        let registry = Registry::from(definition.clone());

        assert_eq!(registry.get(&id).unwrap(), definition);
    }

    #[test]
    fn get_returns_not_found_when_missing() {
        let registry = Registry::from(AgentTypeDefinition::empty_with_metadata(
            AgentTypeID::try_from("ns/agent:0.0.0").unwrap(),
        ));

        let result = registry.get(&AgentTypeID::try_from("ns/missing:0.0.0").unwrap());

        assert_matches!(result, Err(AgentTypeRegistryError::NotFound(name)) => {
            assert_eq!("ns/missing:0.0.0", name);
        });
    }
}
