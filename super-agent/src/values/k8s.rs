use crate::agent_type::agent_values::AgentValues;
use crate::agent_type::definition::AgentType;
use crate::k8s;
use crate::k8s::store::{K8sStore, STORE_KEY_LOCAL_DATA_CONFIG, STORE_KEY_OPAMP_DATA_CONFIG};
use crate::super_agent::config::AgentID;
use crate::values::values_repository::{ValuesRepository, ValuesRepositoryError};
use std::sync::Arc;
use thiserror::Error;
use tracing::debug;

pub struct ValuesRepositoryConfigMap {
    k8s_store: Arc<K8sStore>,
    remote_enabled: bool,
}

#[derive(Error, Debug)]
pub enum K8sValuesRepositoryError {
    #[error("error from k8s storer while loading SubAgentConfig: {0}")]
    FailedToPersistK8s(#[from] k8s::Error),
    #[cfg(test)]
    #[error("common variant for k8s and on-host implementations")]
    Generic,
}

impl ValuesRepositoryConfigMap {
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

impl ValuesRepository for ValuesRepositoryConfigMap {
    fn load_local(&self, agent_id: &AgentID) -> Result<Option<AgentValues>, ValuesRepositoryError> {
        self.k8s_store
            .get_local_data::<AgentValues>(agent_id, STORE_KEY_LOCAL_DATA_CONFIG)
            .map_err(|err| ValuesRepositoryError::LoadError(err.to_string()))
    }

    fn load_remote(
        &self,
        agent_id: &AgentID,
        agent_type: &AgentType,
    ) -> Result<Option<AgentValues>, ValuesRepositoryError> {
        if !self.remote_enabled || !agent_type.has_remote_management() {
            return Ok(None);
        }

        self.k8s_store
            .get_opamp_data::<AgentValues>(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|err| ValuesRepositoryError::LoadError(err.to_string()))
    }

    fn store_remote(
        &self,
        agent_id: &AgentID,
        agent_values: &AgentValues,
    ) -> Result<(), ValuesRepositoryError> {
        debug!(agent_id = agent_id.to_string(), "saving remote config");

        self.k8s_store
            .set_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_CONFIG, agent_values)
            .map_err(|err| ValuesRepositoryError::StoreError(err.to_string()))?;
        Ok(())
    }

    fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ValuesRepositoryError> {
        debug!(agent_id = agent_id.to_string(), "deleting remote config");

        self.k8s_store
            .delete_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|err| ValuesRepositoryError::DeleteError(err.to_string()))?;
        Ok(())
    }
}
