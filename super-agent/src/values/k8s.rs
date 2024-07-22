use crate::k8s;
use crate::k8s::store::{K8sStore, STORE_KEY_LOCAL_DATA_CONFIG, STORE_KEY_OPAMP_DATA_CONFIG};
use crate::super_agent::config::AgentID;
use crate::values::yaml_config::{has_remote_management, YAMLConfig};
use crate::values::yaml_config_repository::{YAMLConfigRepository, YAMLConfigRepositoryError};
use opamp_client::operation::capabilities::Capabilities;
use std::sync::Arc;
use thiserror::Error;
use tracing::debug;

pub struct YAMLConfigRepositoryConfigMap {
    k8s_store: Arc<K8sStore>,
    remote_enabled: bool,
}

#[derive(Error, Debug)]
pub enum K8sYAMLConfigRepositoryError {
    #[error("error from k8s storer while loading SubAgentConfig: {0}")]
    FailedToPersistK8s(#[from] k8s::Error),
    #[cfg(test)]
    #[error("common variant for k8s and on-host implementations")]
    Generic,
}

impl YAMLConfigRepositoryConfigMap {
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

impl YAMLConfigRepository for YAMLConfigRepositoryConfigMap {
    fn load_local(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<YAMLConfig>, YAMLConfigRepositoryError> {
        self.k8s_store
            .get_local_data::<YAMLConfig>(agent_id, STORE_KEY_LOCAL_DATA_CONFIG)
            .map_err(|err| YAMLConfigRepositoryError::LoadError(err.to_string()))
    }

    fn load_remote(
        &self,
        agent_id: &AgentID,
        capabilities: &Capabilities,
    ) -> Result<Option<YAMLConfig>, YAMLConfigRepositoryError> {
        if !self.remote_enabled || !has_remote_management(capabilities) {
            return Ok(None);
        }

        self.k8s_store
            .get_opamp_data::<YAMLConfig>(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|err| YAMLConfigRepositoryError::LoadError(err.to_string()))
    }

    fn store_remote(
        &self,
        agent_id: &AgentID,
        yaml_config: &YAMLConfig,
    ) -> Result<(), YAMLConfigRepositoryError> {
        debug!(agent_id = agent_id.to_string(), "saving remote config");

        self.k8s_store
            .set_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_CONFIG, yaml_config)
            .map_err(|err| YAMLConfigRepositoryError::StoreError(err.to_string()))?;
        Ok(())
    }

    fn delete_remote(&self, agent_id: &AgentID) -> Result<(), YAMLConfigRepositoryError> {
        debug!(agent_id = agent_id.to_string(), "deleting remote config");

        self.k8s_store
            .delete_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|err| YAMLConfigRepositoryError::DeleteError(err.to_string()))?;
        Ok(())
    }
}
