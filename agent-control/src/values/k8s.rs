use crate::agent_control::agent_id::AgentID;
use crate::k8s;
use crate::k8s::store::{K8sStore, STORE_KEY_LOCAL_DATA_CONFIG, STORE_KEY_OPAMP_DATA_CONFIG};
use crate::opamp::remote_config::hash::Hash;
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
            .map_err(|err| ConfigRepositoryError::LoadError(err.to_string()))?;

        match maybe_yaml_config {
            Some(yaml_config) => Ok(Some(Config::LocalConfig(yaml_config))),
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
            .map_err(|err| ConfigRepositoryError::LoadError(err.to_string()))?;

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
            .map_err(|err| ConfigRepositoryError::StoreError(err.to_string()))?;
        Ok(())
    }

    fn get_hash(&self, agent_id: &AgentID) -> Result<Option<Hash>, ConfigRepositoryError> {
        let remote_config = self
            .k8s_store
            .get_opamp_data::<RemoteConfig>(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|err| ConfigRepositoryError::LoadError(err.to_string()))?;

        if let Some(rc) = remote_config {
            return Ok(Some(rc.config_hash));
        }

        Ok(None)
    }

    fn update_hash(&self, agent_id: &AgentID, hash: &Hash) -> Result<(), ConfigRepositoryError> {
        debug!(
            agent_id = agent_id.to_string(),
            "updating remote config hash"
        );

        let maybe_yaml_config = self
            .k8s_store
            .get_opamp_data::<RemoteConfig>(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|err| ConfigRepositoryError::LoadError(err.to_string()))?;

        match maybe_yaml_config {
            Some(mut remote_config) => {
                remote_config.config_hash = hash.clone();
                self.k8s_store
                    .set_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_CONFIG, &remote_config)
                    .map_err(|err| ConfigRepositoryError::StoreError(err.to_string()))?;
                Ok(())
            }
            None => Err(ConfigRepositoryError::UpdateHashError),
        }
    }

    #[tracing::instrument(skip_all, err)]
    fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ConfigRepositoryError> {
        debug!(agent_id = agent_id.to_string(), "deleting remote config");

        self.k8s_store
            .delete_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|err| ConfigRepositoryError::DeleteError(err.to_string()))?;
        Ok(())
    }
}
