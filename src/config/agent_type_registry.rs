use std::collections::HashMap;

use thiserror::Error;

use super::agent_type::{Agent, RawAgent};

#[derive(Error, Debug)]
pub enum AgentRepositoryError {
    #[error("agent not found")]
    NotFound,
    #[error("`{0}`")]
    SerdeYaml(#[from] serde_yaml::Error),
}

/// AgentRegistry stores and loads Agent types.
trait AgentRepository {
    // get returns an AgentType given a definition.
    fn get(&self, name: &str) -> Result<&Agent, AgentRepositoryError>;
}

struct LocalRepository(HashMap<String, Agent>);

impl AgentRepository for LocalRepository {
    fn get(&self, name: &str) -> Result<&Agent, AgentRepositoryError> {
        self.0.get(name).ok_or(AgentRepositoryError::NotFound)
    }
}

impl LocalRepository {
    pub(crate) fn new() -> Self {
        LocalRepository(HashMap::new())
    }

    fn add_source<R>(&mut self, reader: R) -> Result<(), AgentRepositoryError>
    where
        R: std::io::Read,
    {
        let raw_agent: RawAgent = serde_yaml::from_reader(reader)?;
        self.0
            .insert(raw_agent.name.clone(), Agent::from(raw_agent));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::config::agent_type::tests::{AGENT_GIVEN_BAD_YAML, AGENT_GIVEN_YAML};

    use super::*;

    #[test]
    fn add_multiple_agents() {
        let mut repository = LocalRepository::new();

        assert!(repository.add_source(AGENT_GIVEN_YAML.as_bytes()).is_ok());
        assert!(repository
            .add_source(AGENT_GIVEN_BAD_YAML.as_bytes())
            .is_err());

        assert_eq!(repository.get("nrdot").unwrap().name, "nrdot");

        let invalid_lookup = repository.get("not_an_agent");
        assert!(invalid_lookup.is_err());

        assert_eq!(
            invalid_lookup.unwrap_err().to_string(),
            "agent not found".to_string()
        )
    }
}
