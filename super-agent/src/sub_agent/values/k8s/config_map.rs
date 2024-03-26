use crate::agent_type::agent_values::AgentValues;
use crate::agent_type::definition::AgentType;
use crate::k8s;
use crate::k8s::store::{K8sStore, STORE_KEY_LOCAL_DATA_CONFIG, STORE_KEY_OPAMP_DATA_CONFIG};
use crate::sub_agent::values::values_repository::ValuesRepository;
use crate::super_agent::config::AgentID;
use std::sync::Arc;
use thiserror::Error;
use tracing::debug;

pub struct ValuesRepositoryConfigMap {
    k8s_store: Arc<K8sStore>,
    remote_enabled: bool,
}

#[derive(Error, Debug)]
pub enum ValuesRepositoryError {
    #[error("error from k8s storer: {0}")]
    FailedToPersistK8s(#[from] k8s::Error),
    #[error("serialize error on store: `{0}`")]
    StoreSerializeError(#[from] serde_yaml::Error),
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
    // load(...) looks for remote configs first, if unavailable checks the local ones.
    // If none is found, it fallbacks to the default values.
    fn load(
        &self,
        agent_id: &AgentID,
        agent_type: &AgentType,
    ) -> Result<AgentValues, ValuesRepositoryError> {
        debug!(agent_id = agent_id.to_string(), "load config");

        if self.remote_enabled && agent_type.has_remote_management() {
            if let Some(values_result) = self
                .k8s_store
                .get_opamp_data::<AgentValues>(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)?
            {
                return Ok(values_result);
            }
            debug!(agent_id = agent_id.to_string(), "remote config not found");
        }

        if let Some(values_result) = self
            .k8s_store
            .get_local_data::<AgentValues>(agent_id, STORE_KEY_LOCAL_DATA_CONFIG)?
        {
            return Ok(values_result);
        }

        debug!(agent_id = agent_id.to_string(), "local config not found, falling back to defaults");
        Ok(AgentValues::default())
    }

    fn store_remote(
        &self,
        agent_id: &AgentID,
        agent_values: &AgentValues,
    ) -> Result<(), ValuesRepositoryError> {
        debug!(agent_id = agent_id.to_string(), "saving remote config");

        self.k8s_store
            .set_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_CONFIG, agent_values)?;
        Ok(())
    }

    fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ValuesRepositoryError> {
        debug!(agent_id = agent_id.to_string(), "delete remote config");

        self.k8s_store
            .delete_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)?;
        Ok(())
    }
}
