use crate::super_agent::defaults::{
    DYNAMIC_AGENT_TYPE, NEWRELIC_INFRA_TYPE_0_0_1, NEWRELIC_INFRA_TYPE_0_0_2,
    NEWRELIC_INFRA_TYPE_0_1_0, NEWRELIC_INFRA_TYPE_0_1_1, NRDOT_TYPE_0_0_1, NRDOT_TYPE_0_1_0,
};
use std::collections::HashMap;
use thiserror::Error;
use tracing::debug;

use super::definition::AgentTypeDefinition;

#[derive(Error, Debug)]
pub enum AgentRepositoryError {
    #[error("agent not found")]
    NotFound,
    #[error("agent `{0}` already exists")]
    AlreadyExists(String),
    #[error("`{0}`")]
    SerdeYaml(#[from] serde_yaml::Error),
}

/// AgentRegistry stores and loads Agent types.
pub trait AgentRegistry {
    // get returns an Agent type given a definition.
    // TODO: evaluate if returning an owned value is needed, CoW?
    fn get(&self, name: &str) -> Result<AgentTypeDefinition, AgentRepositoryError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalRegistry(HashMap<String, AgentTypeDefinition>);

impl Default for LocalRegistry {
    // default returns the LocalRegistry loaded with the defined default agents
    fn default() -> Self {
        let mut local_agent_type_repository = LocalRegistry(HashMap::new());
        // save to unwrap(), default agent cannot be changed inline
        local_agent_type_repository
            .store_from_yaml(NEWRELIC_INFRA_TYPE_0_0_1.as_bytes())
            .unwrap();
        local_agent_type_repository
            .store_from_yaml(NEWRELIC_INFRA_TYPE_0_0_2.as_bytes())
            .unwrap();
        local_agent_type_repository
            .store_from_yaml(NEWRELIC_INFRA_TYPE_0_1_0.as_bytes())
            .unwrap();
        local_agent_type_repository
            .store_from_yaml(NEWRELIC_INFRA_TYPE_0_1_1.as_bytes())
            .unwrap();
        local_agent_type_repository
            .store_from_yaml(NRDOT_TYPE_0_0_1.as_bytes())
            .unwrap();
        local_agent_type_repository
            .store_from_yaml(NRDOT_TYPE_0_1_0.as_bytes())
            .unwrap();

        if let Ok(file) = std::fs::read_to_string(DYNAMIC_AGENT_TYPE) {
            _ = local_agent_type_repository
                .store_from_yaml(file.as_bytes())
                .inspect_err(|e| debug!("Could not add dynamic-agent-type.yaml: {e}"));
        }

        local_agent_type_repository
    }
}

impl LocalRegistry {
    pub fn store_from_yaml(&mut self, agent_bytes: &[u8]) -> Result<(), AgentRepositoryError> {
        let agent: AgentTypeDefinition = serde_yaml::from_reader(agent_bytes)?;
        //  We check if an agent already exists and fail if so.
        let metadata = agent.metadata.to_string();
        if self.0.get(&metadata).is_some() {
            return Err(AgentRepositoryError::AlreadyExists(metadata));
        }
        self.0.insert(metadata, agent);
        Ok(())
    }
}

impl AgentRegistry for LocalRegistry {
    fn get(&self, name: &str) -> Result<AgentTypeDefinition, AgentRepositoryError> {
        match self.0.get(name) {
            None => Err(AgentRepositoryError::NotFound),
            Some(final_agent) => Ok(final_agent.clone()),
        }
    }
}

impl LocalRegistry {
    pub fn new<A: IntoIterator<Item = AgentTypeDefinition>>(agents: A) -> Self {
        let mut registry = LocalRegistry::default();

        for agent in agents {
            registry.0.insert(agent.metadata.to_string(), agent);
        }

        registry
    }
}

#[cfg(test)]
pub mod tests {
    use crate::agent_type::definition::tests::AGENT_GIVEN_YAML;

    use super::*;
    use mockall::{mock, predicate};

    // Mock
    mock! {
        pub AgentRegistryMock {}

        impl AgentRegistry for AgentRegistryMock  {
            fn get(&self, name: &str) -> Result<AgentTypeDefinition, AgentRepositoryError>;
        }
    }

    impl MockAgentRegistryMock {
        pub fn should_get(&mut self, name: String, final_agent: &AgentTypeDefinition) {
            let final_agent = final_agent.clone();
            self.expect_get()
                .with(predicate::eq(name.clone()))
                .once()
                .returning(move |_| Ok(final_agent.clone()));
        }

        pub fn should_not_get(&mut self, name: String) {
            self.expect_get()
                .with(predicate::eq(name.clone()))
                .once()
                .returning(move |_| Err(AgentRepositoryError::NotFound));
        }
    }

    impl LocalRegistry {
        pub fn store_with_key(
            &mut self,
            key: String,
            agent: AgentTypeDefinition,
        ) -> Result<(), AgentRepositoryError> {
            Ok(_ = self.0.insert(key, agent))
        }
    }

    #[test]
    fn default_local_registry() {
        let registry = LocalRegistry::default();
        assert_eq!(registry.0.len(), 6)
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

    #[test]
    fn add_duplicate_agents() {
        let mut repository = LocalRegistry::default();

        assert!(repository
            .store_from_yaml(AGENT_GIVEN_YAML.as_bytes())
            .is_ok());

        let duplicate = repository.store_from_yaml(AGENT_GIVEN_YAML.as_bytes());
        assert!(duplicate.is_err());

        assert_eq!(
            duplicate.unwrap_err().to_string(),
            "agent `newrelic/nrdot:0.1.0` already exists".to_string()
        )
    }
}
