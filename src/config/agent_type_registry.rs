use std::collections::HashMap;

use thiserror::Error;

use super::agent_type::{Agent, AgentName};

#[derive(Error, Debug)]
pub enum AgentRepositoryError {
    #[error("agent not found")]
    NotFound,
    #[error("`{0}`")]
    SerdeYaml(#[from] serde_yaml::Error),
}

/// AgentRegistry stores and loads Agent types.
trait AgentRepository {
    // get returns an Agent type given a definition.
    fn get(&self, name: &AgentName) -> Result<&Agent, AgentRepositoryError>;

    // stores a given Agent type.
    fn store(&mut self, agent: Agent) -> Result<(), AgentRepositoryError>;
}

struct LocalRepository(HashMap<String, Agent>);

impl AgentRepository for LocalRepository {
    fn get(&self, name: &AgentName) -> Result<&Agent, AgentRepositoryError> {
        self.0.get(name).ok_or(AgentRepositoryError::NotFound)
    }

    fn store(&mut self, agent: Agent) -> Result<(), AgentRepositoryError> {
        self.0.insert(agent.name.clone(), agent);
        Ok(())
    }
}

impl LocalRepository {
    pub(crate) fn new() -> Self {
        LocalRepository(HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use crate::config::agent_type::tests::AGENT_GIVEN_YAML;

    use super::*;

    fn retrieve_agent<R>(reader: R) -> super::Agent
    where
        R: std::io::Read,
    {
        serde_yaml::from_reader(reader).unwrap()
    }

    #[test]
    fn add_multiple_agents() {
        let mut repository = LocalRepository::new();

        assert!(repository
            .store(retrieve_agent(AGENT_GIVEN_YAML.as_bytes()))
            .is_ok());

        assert_eq!(repository.get(&"nrdot".to_string()).unwrap().name, "nrdot");

        let invalid_lookup = repository.get(&"not_an_agent".to_string());
        assert!(invalid_lookup.is_err());

        assert_eq!(
            invalid_lookup.unwrap_err().to_string(),
            "agent not found".to_string()
        )
    }
}
