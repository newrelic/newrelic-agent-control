use std::collections::HashMap;

use thiserror::Error;

use super::agent_type::Agent;

#[derive(Error, Debug)]
pub enum AgentRepositoryError {
    #[error("agent not found")]
    NotFound,
    #[error("`{0}`")]
    SerdeYaml(#[from] serde_yaml::Error),
}

/// AgentRegistry stores and loads Agent types.
pub trait AgentRepository {
    // get returns an Agent type given a definition.
    fn get(&self, name: &str) -> Result<&Agent, AgentRepositoryError>;

    // stores a given Agent type.
    fn store_from_yaml(&mut self, agent_bytes: &[u8]) -> Result<(), AgentRepositoryError>;

    fn store_with_key(&mut self, key: String, agent: Agent) -> Result<(), AgentRepositoryError>;
}

#[derive(Debug, Default, Clone)]
pub struct LocalRepository(HashMap<String, Agent>);

impl AgentRepository for LocalRepository {
    fn get(&self, name: &str) -> Result<&Agent, AgentRepositoryError> {
        self.0.get(name).ok_or(AgentRepositoryError::NotFound)
    }

    fn store_from_yaml(&mut self, agent_bytes: &[u8]) -> Result<(), AgentRepositoryError> {
        let agent: Agent = serde_yaml::from_reader(agent_bytes).unwrap();
        self.0.insert(agent.metadata.to_string(), agent);
        Ok(())
    }

    fn store_with_key(&mut self, key: String, agent: Agent) -> Result<(), AgentRepositoryError> {
        Ok(_ = self.0.insert(key, agent))
    }
}

impl LocalRepository {
    pub fn new() -> Self {
        LocalRepository::default()
    }
}

#[cfg(test)]
mod tests {
    use crate::config::agent_type::tests::AGENT_GIVEN_YAML;

    use super::*;

    fn retrieve_agent<R>(reader: R) -> Agent
    where
        R: std::io::Read,
    {
        serde_yaml::from_reader(reader).unwrap()
    }

    #[test]
    fn add_multiple_agents() {
        let mut repository = LocalRepository::new();

        assert!(repository
            .store_from_yaml(AGENT_GIVEN_YAML.as_bytes())
            .is_ok());

        assert_eq!(
            repository
                .get(&"newrelic/nrdot:0.1.0".to_string())
                .unwrap()
                .metadata
                .to_string(),
            "newrelic/nrdot:0.1.0"
        );

        let invalid_lookup = repository.get(&"not_an_agent".to_string());
        assert!(invalid_lookup.is_err());

        assert_eq!(
            invalid_lookup.unwrap_err().to_string(),
            "agent not found".to_string()
        )
    }
}
