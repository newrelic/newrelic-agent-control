use std::collections::HashMap;

use thiserror::Error;

use crate::agent::defaults::{NEWRELIC_INFRA_TYPE, NRDOT_TYPE};

use super::agent_type::agent_types::FinalAgent;

#[derive(Error, Debug)]
pub enum AgentRepositoryError {
    #[error("agent not found")]
    NotFound,
    #[error("`{0}`")]
    SerdeYaml(#[from] serde_yaml::Error),
}

/// AgentRegistry stores and loads Agent types.
pub trait AgentRegistry {
    // get returns an Agent type given a definition.
    fn get(&self, name: &str) -> Result<&FinalAgent, AgentRepositoryError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalRegistry(HashMap<String, FinalAgent>);

impl Default for LocalRegistry {
    // default returns the LocalRegistry loaded with the defined default agents
    fn default() -> Self {
        let mut local_agent_type_repository = LocalRegistry(HashMap::new());
        // save to unwrap(), default agent cannot be changed inline
        local_agent_type_repository
            .store_from_yaml(NEWRELIC_INFRA_TYPE.as_bytes())
            .unwrap();
        local_agent_type_repository
            .store_from_yaml(NRDOT_TYPE.as_bytes())
            .unwrap();

        local_agent_type_repository
    }
}

impl LocalRegistry {
    pub fn store_from_yaml(&mut self, agent_bytes: &[u8]) -> Result<(), AgentRepositoryError> {
        let agent: FinalAgent = serde_yaml::from_reader(agent_bytes)?;
        self.0.insert(agent.metadata.to_string(), agent);
        Ok(())
    }
}

impl AgentRegistry for LocalRegistry {
    fn get(&self, name: &str) -> Result<&FinalAgent, AgentRepositoryError> {
        self.0.get(name).ok_or(AgentRepositoryError::NotFound)
    }
}

impl LocalRegistry {
    pub fn new<A: IntoIterator<Item = FinalAgent>>(agents: A) -> Self {
        let mut registry = LocalRegistry::default();

        for agent in agents {
            registry.0.insert(agent.metadata.to_string(), agent);
        }

        registry
    }
}

#[cfg(test)]
mod tests {

    use crate::config::agent_type::agent_types::tests::AGENT_GIVEN_YAML;

    use super::*;
    impl LocalRegistry {
        pub fn store_with_key(
            &mut self,
            key: String,
            agent: FinalAgent,
        ) -> Result<(), AgentRepositoryError> {
            Ok(_ = self.0.insert(key, agent))
        }
    }

    #[test]
    fn default_local_registry() {
        let registry = LocalRegistry::default();
        assert_eq!(registry.0.len(), 2)
    }

    #[test]
    fn add_multiple_agents() {
        let mut repository = LocalRegistry::default();

        assert!(repository
            .store_from_yaml(AGENT_GIVEN_YAML.as_bytes())
            .is_ok());

        assert_eq!(
            repository
                .get("newrelic/nrdot:0.1.0")
                .unwrap()
                .metadata
                .to_string(),
            "newrelic/nrdot:0.1.0"
        );

        let invalid_lookup = repository.get("not_an_agent");
        assert!(invalid_lookup.is_err());

        assert_eq!(
            invalid_lookup.unwrap_err().to_string(),
            "agent not found".to_string()
        )
    }
}
