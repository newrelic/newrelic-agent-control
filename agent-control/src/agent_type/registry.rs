pub mod embedded;

use thiserror::Error;

use super::definition::AgentTypeDefinition;

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

/// AgentTypeRegistry stores and loads Agent types.
pub trait AgentTypeRegistry {
    /// Returns an Agent type given a definition.
    fn get(&self, agent_type_id: &str) -> Result<AgentTypeDefinition, AgentTypeRegistryError>;

    /// Returns true if the registry has an Agent type for the given id.
    fn contains(&self, agent_type_id: &str) -> bool;
}

#[cfg(test)]
pub mod tests {

    use super::*;
    use mockall::{mock, predicate};

    // Mock
    mock! {
        pub AgentTypeRegistry {}

        impl AgentTypeRegistry for AgentTypeRegistry  {
            fn get(&self, agent_type_id: &str) -> Result<AgentTypeDefinition, AgentTypeRegistryError>;
            fn contains(&self, agent_type_id: &str) -> bool;
        }
    }

    impl MockAgentTypeRegistry {
        pub fn should_get(&mut self, agent_type_id: String, final_agent: &AgentTypeDefinition) {
            let final_agent = final_agent.clone();
            self.expect_get()
                .with(predicate::eq(agent_type_id))
                .once()
                .returning(move |_| Ok(final_agent.clone()));
        }

        pub fn should_not_get(&mut self, agent_type_id: String) {
            self.expect_get()
                .with(predicate::eq(agent_type_id.clone()))
                .once()
                .returning(move |_| Err(AgentTypeRegistryError::NotFound(agent_type_id.clone())));
        }

        pub fn should_contain(&mut self, agent_type_id: String) {
            self.expect_contains()
                .with(predicate::eq(agent_type_id))
                .once()
                .returning(|_| true);
        }

        pub fn should_not_contain(&mut self, agent_type_id: String) {
            self.expect_contains()
                .with(predicate::eq(agent_type_id))
                .once()
                .returning(|_| false);
        }
    }
}
