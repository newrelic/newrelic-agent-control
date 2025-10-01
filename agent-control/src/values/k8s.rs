use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{STORE_KEY_LOCAL_DATA_CONFIG, STORE_KEY_OPAMP_DATA_CONFIG};
use crate::k8s;
use crate::k8s::store::K8sStore;
use crate::opamp::remote_config::hash::ConfigState;
use crate::values::config::{Config, RemoteConfig};
use crate::values::config_repository::{ConfigRepository, ConfigRepositoryError};
use crate::values::yaml_config::{YAMLConfig, has_remote_management};
use opamp_client::operation::capabilities::Capabilities;
use std::sync::Arc;
use thiserror::Error;
use tracing::debug;

pub struct ConfigRepositoryConfigMap {
    k8s_store: Arc<K8sStore>,
    remote_enabled: bool,
}

#[derive(Error, Debug)]
pub enum K8sConfigRepositoryError {
    #[error("error from k8s storer while loading SubAgentConfig: {0}")]
    FailedToPersistK8s(#[from] k8s::Error),
    #[cfg(test)]
    #[error("common variant for k8s and on-host implementations")]
    Generic,
}

impl ConfigRepositoryConfigMap {
    pub fn new(k8s_store: Arc<K8sStore>) -> Self {
        Self {
            k8s_store,
            remote_enabled: false,
        }
    }

    pub fn with_remote(mut self) -> Self {
        self.remote_enabled = true;
        self
    }
}

impl ConfigRepository for ConfigRepositoryConfigMap {
    #[tracing::instrument(skip_all)]
    fn load_local(&self, agent_id: &AgentID) -> Result<Option<Config>, ConfigRepositoryError> {
        let maybe_yaml_config = self
            .k8s_store
            .get_local_data::<YAMLConfig>(agent_id, STORE_KEY_LOCAL_DATA_CONFIG)
            .map_err(|err| {
                ConfigRepositoryError::LoadError(format!("loading local config: {err}"))
            })?;

        match maybe_yaml_config {
            Some(yaml_config) => Ok(Some(Config::LocalConfig(yaml_config.into()))),
            None => Ok(None),
        }
    }

    #[tracing::instrument(skip_all)]
    fn load_remote(
        &self,
        agent_id: &AgentID,
        capabilities: &Capabilities,
    ) -> Result<Option<Config>, ConfigRepositoryError> {
        if !self.remote_enabled || !has_remote_management(capabilities) {
            return Ok(None);
        }

        let maybe_remote_config = self
            .k8s_store
            .get_opamp_data::<RemoteConfig>(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|err| {
                ConfigRepositoryError::LoadError(format!("loading remote config: {err}"))
            })?;

        match maybe_remote_config {
            Some(remote_config) => Ok(Some(Config::RemoteConfig(remote_config))),
            None => Ok(None),
        }
    }

    #[tracing::instrument(skip_all)]
    fn store_remote(
        &self,
        agent_id: &AgentID,
        remote_config: &RemoteConfig,
    ) -> Result<(), ConfigRepositoryError> {
        debug!(agent_id = agent_id.to_string(), "saving remote config");

        self.k8s_store
            .set_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_CONFIG, remote_config)
            .map_err(|err| {
                ConfigRepositoryError::StoreError(format!("storing remote config: {err}"))
            })?;
        Ok(())
    }

    fn get_remote_config(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<RemoteConfig>, ConfigRepositoryError> {
        self.k8s_store
            .get_opamp_data::<RemoteConfig>(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|err| {
                ConfigRepositoryError::LoadError(format!("getting remote config hash: {err}"))
            })
    }

    fn update_state(
        &self,
        agent_id: &AgentID,
        state: ConfigState,
    ) -> Result<(), ConfigRepositoryError> {
        debug!(
            agent_id = agent_id.to_string(),
            "updating remote config hash"
        );

        let maybe_config = self
            .k8s_store
            .get_opamp_data::<RemoteConfig>(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|err| {
                ConfigRepositoryError::LoadError(format!("updating remote config state: {err}"))
            })?;

        match maybe_config {
            Some(remote_config) => {
                self.k8s_store
                    .set_opamp_data(
                        agent_id,
                        STORE_KEY_OPAMP_DATA_CONFIG,
                        &remote_config.with_state(state),
                    )
                    .map_err(|err| {
                        ConfigRepositoryError::StoreError(format!(
                            "updating remote config state: {err}"
                        ))
                    })?;
                Ok(())
            }
            None => Err(ConfigRepositoryError::UpdateHashStateError(
                "No remote config found".to_string(),
            )),
        }
    }

    #[tracing::instrument(skip_all, err)]
    fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ConfigRepositoryError> {
        debug!(agent_id = agent_id.to_string(), "deleting remote config");

        self.k8s_store
            .delete_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|err| {
                ConfigRepositoryError::DeleteError(format!("deleting remote config: {err}"))
            })?;
        Ok(())
    }
}
