use crate::agent_control::agent_id::AgentID;
use crate::values::config::{Config, RemoteConfig};

use crate::opamp::remote_config::hash::{ConfigState, Hash};
use opamp_client::operation::capabilities::Capabilities;
use thiserror::Error;
use tracing::debug;

#[derive(Error, Debug, Clone)]
pub enum ConfigRepositoryError {
    #[error("error loading values: `{0}`")]
    LoadError(String),
    #[error("error storing values: `{0}`")]
    StoreError(String),
    #[error("error deleting values: `{0}`")]
    DeleteError(String),
    #[error("error updating hash, no remote config to update: `{0}`")]
    UpdateHashStateError(String),
}

pub trait ConfigRepository: Send + Sync + 'static {
    fn load_local(&self, agent_id: &AgentID) -> Result<Option<Config>, ConfigRepositoryError>;

    fn load_remote(
        &self,
        agent_id: &AgentID,
        capabilities: &Capabilities,
    ) -> Result<Option<Config>, ConfigRepositoryError>;

    fn store_remote(
        &self,
        agent_id: &AgentID,
        remote_config: &RemoteConfig,
    ) -> Result<(), ConfigRepositoryError>;

    fn get_hash(&self, agent_id: &AgentID) -> Result<Option<Hash>, ConfigRepositoryError>;

    fn update_hash_state(
        &self,
        agent_id: &AgentID,
        state: &ConfigState,
    ) -> Result<(), ConfigRepositoryError>;

    fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ConfigRepositoryError>;
}

/// Looks for remote configs first, if unavailable checks the local ones.
/// It returns none if no configuration is found.
pub fn load_remote_fallback_local<R: ConfigRepository>(
    config_repository: &R,
    agent_id: &AgentID,
    capabilities: &Capabilities,
) -> Result<Option<Config>, ConfigRepositoryError> {
    debug!("loading config");

    if let remote @ Some(_) = config_repository.load_remote(agent_id, capabilities)? {
        return Ok(remote);
    }
    debug!("remote config not found, loading local");

    config_repository.load_local(agent_id)
}
#[cfg(test)]
pub mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::load_remote_fallback_local;
    use crate::agent_control::agent_id::AgentID;
    use crate::opamp::remote_config::hash::{ConfigState, Hash};
    use crate::values::config::{Config, RemoteConfig};
    use crate::values::config_repository::{ConfigRepository, ConfigRepositoryError};
    use crate::values::yaml_config::YAMLConfig;
    use mockall::{mock, predicate};
    use opamp_client::operation::capabilities::Capabilities;

    #[derive(Debug, Default)]
    pub struct InMemoryConfigRepository {
        local_config: Mutex<HashMap<AgentID, Config>>,
        remote_config: Mutex<HashMap<AgentID, Config>>,
    }
    impl InMemoryConfigRepository {
        pub fn store_local(
            &self,
            agent_id: &AgentID,
            yaml_config: &YAMLConfig,
        ) -> Result<(), ConfigRepositoryError> {
            self.local_config.lock().unwrap().insert(
                agent_id.clone(),
                Config::LocalConfig(yaml_config.clone().into()),
            );
            Ok(())
        }
        pub fn assert_no_config_for_agent(&self, agent_id: &AgentID) {
            assert!(
                load_remote_fallback_local(self, agent_id, &Capabilities::default())
                    .unwrap()
                    .is_none()
            );
        }
    }

    impl ConfigRepository for InMemoryConfigRepository {
        fn store_remote(
            &self,
            agent_id: &AgentID,
            remote_config: &RemoteConfig,
        ) -> Result<(), ConfigRepositoryError> {
            self.remote_config.lock().unwrap().insert(
                agent_id.clone(),
                Config::RemoteConfig(remote_config.clone()),
            );
            Ok(())
        }

        fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ConfigRepositoryError> {
            self.remote_config.lock().unwrap().remove(agent_id);
            Ok(())
        }

        fn load_local(&self, agent_id: &AgentID) -> Result<Option<Config>, ConfigRepositoryError> {
            Ok(self
                .local_config
                .lock()
                .unwrap()
                .get(agent_id)
                .map(|config| Config::LocalConfig(config.get_yaml_config().clone().into())))
        }

        fn load_remote(
            &self,
            agent_id: &AgentID,
            _capabilities: &Capabilities,
        ) -> Result<Option<Config>, ConfigRepositoryError> {
            Ok(self
                .remote_config
                .lock()
                .unwrap()
                .get(agent_id)
                .map(|config| {
                    let remote_config = RemoteConfig::new(
                        config.get_yaml_config().clone(),
                        config.get_hash().unwrap(),
                    );
                    Config::RemoteConfig(remote_config)
                }))
        }

        fn get_hash(&self, agent_id: &AgentID) -> Result<Option<Hash>, ConfigRepositoryError> {
            let binding = self.remote_config.lock().unwrap();
            let remote_config = binding.get(agent_id);

            if let Some(rc) = remote_config {
                return Ok(rc.get_hash());
            }

            Ok(None)
        }

        fn update_hash_state(
            &self,
            agent_id: &AgentID,
            state: &ConfigState,
        ) -> Result<(), ConfigRepositoryError> {
            let updated_remote_config =
                self.remote_config
                    .lock()
                    .unwrap()
                    .get(agent_id)
                    .and_then(|remote_config| {
                        if let Some(mut hash) = remote_config.get_hash() {
                            hash.update_state(state);
                            let remote_config =
                                RemoteConfig::new(remote_config.get_yaml_config().clone(), hash);
                            return Some(Config::RemoteConfig(remote_config));
                        }
                        None
                    });

            if let Some(remote_config) = updated_remote_config {
                self.remote_config
                    .lock()
                    .unwrap()
                    .insert(agent_id.clone(), remote_config);
            }

            Ok(())
        }
    }

    mock! {
        pub(crate) ConfigRepository {}

        impl ConfigRepository for ConfigRepository {
            fn store_remote(
                &self,
                agent_id: &AgentID,
                remote_config: &RemoteConfig,
            ) -> Result<(), ConfigRepositoryError>;

            fn get_hash(
                &self,
                agent_id: &AgentID,
            ) -> Result<Option<Hash>, ConfigRepositoryError>;

            fn update_hash_state(
                &self,
                agent_id: &AgentID,
                state: &ConfigState,
            ) -> Result<(), ConfigRepositoryError>;

            fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ConfigRepositoryError>;

            fn load_local(
                &self,
                agent_id: &AgentID,
            ) -> Result<Option<Config>, ConfigRepositoryError>;

            fn load_remote(
                &self,
                agent_id: &AgentID,
                capabilities: &Capabilities,
            ) -> Result<Option<Config>, ConfigRepositoryError>;
        }
    }

    impl MockConfigRepository {
        pub fn should_load_remote(
            &mut self,
            agent_id: &AgentID,
            capabilities: Capabilities,
            remote_config: RemoteConfig,
        ) {
            self.expect_load_remote()
                .once()
                .with(predicate::eq(agent_id.clone()), predicate::eq(capabilities))
                .returning(move |_, _| Ok(Some(Config::RemoteConfig(remote_config.clone()))));
        }

        pub fn should_not_load_remote(&mut self, agent_id: &AgentID, capabilities: Capabilities) {
            self.expect_load_remote()
                .once()
                .with(predicate::eq(agent_id.clone()), predicate::eq(capabilities))
                .returning(move |_, _| {
                    Err(ConfigRepositoryError::LoadError("load error".to_string()))
                });
        }

        #[allow(dead_code)]
        pub fn should_store_remote(&mut self, agent_id: &AgentID, remote_config: &RemoteConfig) {
            self.expect_store_remote()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(remote_config.clone()),
                )
                .returning(|_, _| Ok(()));
        }
    }
}
