use thiserror::Error;
use tracing::error;

use super::definition::AgentTypeDefinition;

#[derive(Error, Debug)]
pub enum AgentRepositoryError {
    #[error("agent type `{0}` not found")]
    NotFound(String),
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

#[cfg(test)]
pub mod tests {

    use super::*;
    use mockall::{mock, predicate};

    // Mock
    mock! {
        pub AgentRegistry {}

        impl AgentRegistry for AgentRegistry  {
            fn get(&self, name: &str) -> Result<AgentTypeDefinition, AgentRepositoryError>;
        }
    }

    impl MockAgentRegistry {
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
                .returning(move |_| Err(AgentRepositoryError::NotFound(name.clone())));
        }
    }
}
