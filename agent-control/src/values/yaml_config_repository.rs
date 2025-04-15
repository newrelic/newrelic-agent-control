use crate::agent_control::agent_id::AgentID;
use crate::values::yaml_config::YAMLConfig;
use opamp_client::operation::capabilities::Capabilities;
use tracing::debug;

#[derive(thiserror::Error, Debug)]
pub enum YAMLConfigRepositoryError {
    #[error("error loading values: `{0}`")]
    LoadError(String),
    #[error("error storing values: `{0}`")]
    StoreError(String),
    #[error("error deleting values: `{0}`")]
    DeleteError(String),
}

pub trait YAMLConfigRepository: Send + Sync + 'static {
    fn load_local(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<YAMLConfig>, YAMLConfigRepositoryError>;

    fn load_remote(
        &self,
        agent_id: &AgentID,
        capabilities: &Capabilities,
    ) -> Result<Option<YAMLConfig>, YAMLConfigRepositoryError>;

    fn store_remote(
        &self,
        agent_id: &AgentID,
        yaml_config: &YAMLConfig,
    ) -> Result<(), YAMLConfigRepositoryError>;

    fn delete_remote(&self, agent_id: &AgentID) -> Result<(), YAMLConfigRepositoryError>;
}

/// Looks for remote configs first, if unavailable checks the local ones.
/// If none is found, it fallbacks to the empty default values.
pub fn load_remote_fallback_local<R: YAMLConfigRepository>(
    config_repository: &R,
    agent_id: &AgentID,
    capabilities: &Capabilities,
) -> Result<Option<YAMLConfig>, YAMLConfigRepositoryError> {
    debug!("loading config");

    if let remote @ Some(_) = config_repository.load_remote(agent_id, capabilities)? {
        return Ok(remote);
    }
    debug!("remote config not found, loading local");

    config_repository.load_local(agent_id)
}
#[cfg(test)]
pub mod tests {
    use crate::agent_control::agent_id::AgentID;
    use crate::values::yaml_config::YAMLConfig;
    use crate::values::yaml_config_repository::{YAMLConfigRepository, YAMLConfigRepositoryError};
    use mockall::{mock, predicate};
    use opamp_client::operation::capabilities::Capabilities;

    mock! {
        pub(crate) YAMLConfigRepositoryMock {}

        impl YAMLConfigRepository for YAMLConfigRepositoryMock {
            fn store_remote(
                &self,
                agent_id: &AgentID,
                yaml_config: &YAMLConfig,
            ) -> Result<(), YAMLConfigRepositoryError>;

            fn delete_remote(&self, agent_id: &AgentID) -> Result<(), YAMLConfigRepositoryError>;

            fn load_local(
                &self,
                agent_id: &AgentID,
            ) -> Result<Option<YAMLConfig>, YAMLConfigRepositoryError>;

            fn load_remote(
                &self,
                agent_id: &AgentID,
                capabilities: &Capabilities,
            ) -> Result<Option<YAMLConfig>, YAMLConfigRepositoryError>;
        }
    }

    impl MockYAMLConfigRepositoryMock {
        pub fn should_load_remote(
            &mut self,
            agent_id: &AgentID,
            capabilities: Capabilities,
            yaml_config: &YAMLConfig,
        ) {
            let yaml_config = yaml_config.clone();
            self.expect_load_remote()
                .once()
                .with(predicate::eq(agent_id.clone()), predicate::eq(capabilities))
                .returning(move |_, _| Ok(Some(yaml_config.clone())));
        }

        pub fn should_not_load_remote(&mut self, agent_id: &AgentID, capabilities: Capabilities) {
            self.expect_load_remote()
                .once()
                .with(predicate::eq(agent_id.clone()), predicate::eq(capabilities))
                .returning(move |_, _| {
                    Err(YAMLConfigRepositoryError::LoadError(
                        "load error".to_string(),
                    ))
                });
        }

        pub fn should_store_remote(&mut self, agent_id: &AgentID, yaml_config: &YAMLConfig) {
            self.expect_store_remote()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(yaml_config.clone()),
                )
                .returning(|_, _| Ok(()));
        }
    }
}
