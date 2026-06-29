//! The [`ConfigRepository`] trait for loading and persisting agent configurations.

use crate::agent_control::agent_id::AgentID;
use crate::resource_ownership::ResourceOwnership;
use crate::values::config::{Config, RemoteConfig};

use crate::opamp::remote_config::hash::ConfigState;
use opamp_client::operation::capabilities::Capabilities;
use thiserror::Error;
use tracing::debug;

/// Errors returned by [`ConfigRepository`] operations.
#[derive(Error, Debug, Clone)]
pub enum ConfigRepositoryError {
    /// Failed to load configuration values.
    #[error("error loading values: {0}")]
    LoadError(String),
    /// Failed to store configuration values.
    #[error("error storing values: {0}")]
    StoreError(String),
    /// Failed to delete configuration values.
    #[error("error deleting values: {0}")]
    DeleteError(String),
    /// Failed to update the hash state because no remote config exists.
    #[error("error updating hash, no remote config to update: {0}")]
    UpdateHashStateError(String),
}

/// Loads, stores, and deletes agent local and remote configurations.
pub trait ConfigRepository: Send + Sync + 'static {
    /// Loads the local configuration for the given agent, if any.
    fn load_local(&self, agent_id: &AgentID) -> Result<Option<Config>, ConfigRepositoryError>;

    /// Loads the remote configuration for the given agent, if remote management is enabled.
    fn load_remote(
        &self,
        agent_id: &AgentID,
        capabilities: &Capabilities,
    ) -> Result<Option<Config>, ConfigRepositoryError>;

    /// Looks for remote configs first, if unavailable checks the local ones.
    /// It returns none if no configuration is found.
    fn load_remote_fallback_local(
        &self,
        agent_id: &AgentID,
        capabilities: &Capabilities,
    ) -> Result<Option<Config>, ConfigRepositoryError> {
        debug!("loading config");

        if let remote @ Some(_) = self.load_remote(agent_id, capabilities)? {
            return Ok(remote);
        }
        debug!("remote config not found, loading local");

        self.load_local(agent_id)
    }

    /// Stores the remote configuration for the given agent.
    fn store_remote(
        &self,
        agent_id: &AgentID,
        ownership: ResourceOwnership,
        remote_config: &RemoteConfig,
    ) -> Result<(), ConfigRepositoryError>;

    /// Returns the stored remote configuration for the given agent, if any.
    fn get_remote_config(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<RemoteConfig>, ConfigRepositoryError>;

    /// Updates the state of the stored remote configuration for the given agent.
    fn update_state(
        &self,
        agent_id: &AgentID,
        ownership: ResourceOwnership,
        state: ConfigState,
    ) -> Result<(), ConfigRepositoryError>;

    /// Deletes the remote configuration for the given agent.
    fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ConfigRepositoryError>;
}

#[cfg(test)]
#[allow(missing_docs)]
pub mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use crate::agent_control::agent_id::AgentID;
    use crate::opamp::remote_config::hash::ConfigState;
    use crate::resource_ownership::ResourceOwnership;
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
                self.load_remote_fallback_local(agent_id, &Capabilities::default())
                    .unwrap()
                    .is_none()
            );
        }
    }

    impl ConfigRepository for InMemoryConfigRepository {
        fn store_remote(
            &self,
            agent_id: &AgentID,
            _ownership: ResourceOwnership,
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
                    let remote_config = RemoteConfig {
                        config: config.get_yaml_config().clone(),
                        hash: config.get_hash().cloned().unwrap(),
                        state: config.get_state().cloned().unwrap(),
                    };
                    Config::RemoteConfig(remote_config)
                }))
        }

        fn update_state(
            &self,
            agent_id: &AgentID,
            _ownership: ResourceOwnership,
            state: ConfigState,
        ) -> Result<(), ConfigRepositoryError> {
            let updated_remote_config =
                self.remote_config
                    .lock()
                    .unwrap()
                    .get(agent_id)
                    .and_then(|remote_config| {
                        if let Some(hash) = remote_config.get_hash().cloned() {
                            let remote_config = RemoteConfig {
                                config: remote_config.get_yaml_config().clone(),
                                hash,
                                state: state.clone(),
                            };
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

        fn get_remote_config(
            &self,
            agent_id: &AgentID,
        ) -> Result<Option<RemoteConfig>, ConfigRepositoryError> {
            Ok(self
                .remote_config
                .lock()
                .unwrap()
                .get(agent_id)
                .cloned()
                .and_then(Option::<RemoteConfig>::from))
        }
    }

    mock! {
        pub(crate) ConfigRepository {}

        impl ConfigRepository for ConfigRepository {
            fn store_remote(
                &self,
                agent_id: &AgentID,
                ownership: ResourceOwnership,
                remote_config: &RemoteConfig,
            ) -> Result<(), ConfigRepositoryError>;

            fn get_remote_config(
                &self,
                agent_id: &AgentID,
            ) -> Result<Option<RemoteConfig>, ConfigRepositoryError>;

            fn update_state(
                &self,
                agent_id: &AgentID,
                ownership: ResourceOwnership,
                state: ConfigState,
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
    }
}
