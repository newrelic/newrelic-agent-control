pub mod embedded;

use thiserror::Error;

use super::agent_type_id::AgentTypeID;
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

/// Defines how to return an [AgentTypeDefinition] given an identifier.
pub trait AgentTypeRegistry {
    /// Returns an Agent type given its id.
    fn get(
        &self,
        agent_type_id: &AgentTypeID,
    ) -> Result<AgentTypeDefinition, AgentTypeRegistryError>;
}

#[cfg(test)]
pub mod tests {

    use super::*;
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
}
