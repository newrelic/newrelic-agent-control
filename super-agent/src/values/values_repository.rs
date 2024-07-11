use crate::super_agent::config::AgentID;
use crate::values::yaml_config::YAMLConfig;
use tracing::debug;

#[derive(thiserror::Error, Debug)]
pub enum ValuesRepositoryError {
    #[error("error loading values: `{0}`")]
    LoadError(String),
    #[error("error storing values: `{0}`")]
    StoreError(String),
    #[error("error deleting values: `{0}`")]
    DeleteError(String),
}

pub trait ValuesRepository {
    /// load(...) looks for remote configs first, if unavailable checks the local ones.
    /// If none is found, it fallbacks to the default values.
    fn load(&self, agent_id: &AgentID) -> Result<YAMLConfig, ValuesRepositoryError> {
        debug!(agent_id = agent_id.to_string(), "loading config");

        if let Some(values_result) = self.load_remote(agent_id)? {
            return Ok(values_result);
        }
        debug!(
            agent_id = agent_id.to_string(),
            "remote config not found, loading local"
        );

        if let Some(values_result) = self.load_local(agent_id)? {
            return Ok(values_result);
        }
        debug!(
            agent_id = agent_id.to_string(),
            "local config not found, falling back to defaults"
        );
        Ok(YAMLConfig::default())
    }

    fn load_local(&self, agent_id: &AgentID) -> Result<Option<YAMLConfig>, ValuesRepositoryError>;

    fn load_remote(&self, agent_id: &AgentID) -> Result<Option<YAMLConfig>, ValuesRepositoryError>;

    fn store_remote(
        &self,
        agent_id: &AgentID,
        agent_values: &YAMLConfig,
    ) -> Result<(), ValuesRepositoryError>;

    fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ValuesRepositoryError>;
}

#[cfg(test)]
pub mod test {
    use crate::super_agent::config::AgentID;
    use crate::values::values_repository::{ValuesRepository, ValuesRepositoryError};
    use crate::values::yaml_config::YAMLConfig;
    use mockall::{mock, predicate};

    mock! {
        pub(crate) RemoteValuesRepositoryMock {}

        impl ValuesRepository for RemoteValuesRepositoryMock {
            fn store_remote(
                &self,
                agent_id: &AgentID,
                agent_values: &YAMLConfig,
            ) -> Result<(), ValuesRepositoryError>;

            fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ValuesRepositoryError>;

            fn load(
                &self,
                agent_id: &AgentID,
            ) -> Result<YAMLConfig, ValuesRepositoryError>;

            fn load_local(
                &self,
                agent_id: &AgentID,
            ) -> Result<Option<YAMLConfig>, ValuesRepositoryError>;

            fn load_remote(
                &self,
                agent_id: &AgentID,
            ) -> Result<Option<YAMLConfig>, ValuesRepositoryError>;
        }
    }

    impl MockRemoteValuesRepositoryMock {
        pub fn should_load(&mut self, agent_id: &AgentID, agent_values: &YAMLConfig) {
            let agent_values = agent_values.clone();
            self.expect_load()
                .once()
                .with(predicate::eq(agent_id.clone()))
                .returning(move |_| Ok(agent_values.clone()));
        }

        pub fn should_not_load(&mut self, agent_id: &AgentID) {
            self.expect_load()
                .once()
                .with(predicate::eq(agent_id.clone()))
                .returning(move |_| {
                    Err(ValuesRepositoryError::LoadError("load error".to_string()))
                });
        }

        pub fn should_store_remote(&mut self, agent_id: &AgentID, agent_values: &YAMLConfig) {
            self.expect_store_remote()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(agent_values.clone()),
                )
                .returning(|_, _| Ok(()));
        }

        pub fn should_delete_remote(&mut self, agent_id: &AgentID) {
            self.expect_delete_remote()
                .once()
                .with(predicate::eq(agent_id.clone()))
                .returning(|_| Ok(()));
        }
    }
}
