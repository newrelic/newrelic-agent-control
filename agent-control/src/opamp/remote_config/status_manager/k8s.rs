use std::sync::Arc;

use opamp_client::operation::capabilities::Capabilities;
use tracing::debug;

use crate::{
    agent_control::config::AgentID,
    k8s::store::{
        K8sStore, STORE_KEY_LOCAL_DATA_CONFIG, STORE_KEY_OPAMP_DATA_REMOTE_CONFIG_STATUS,
    },
    opamp::remote_config::status::AgentRemoteConfigStatus,
    values::yaml_config::{has_remote_management, YAMLConfig},
};

use super::{error::ConfigStatusManagerError, ConfigStatusManager};

pub struct K8sConfigStatusManager {
    k8s_store: Arc<K8sStore>,
    remote_enabled: bool,
}

impl K8sConfigStatusManager {
    pub fn new(k8s_store: Arc<K8sStore>) -> Self {
        Self {
            k8s_store,
            remote_enabled: false,
        }
    }

    pub fn with_remote(self) -> Self {
        Self {
            remote_enabled: true,
            ..self
        }
    }
}

impl ConfigStatusManager for K8sConfigStatusManager {
    fn retrieve_local_config(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<YAMLConfig>, ConfigStatusManagerError> {
        self.k8s_store
            .get_local_data::<YAMLConfig>(agent_id, STORE_KEY_LOCAL_DATA_CONFIG)
            .map_err(|err| ConfigStatusManagerError::Retrieval(err.to_string()))
    }

    fn retrieve_remote_status(
        &self,
        agent_id: &AgentID,
        capabilities: &Capabilities,
    ) -> Result<Option<AgentRemoteConfigStatus>, ConfigStatusManagerError> {
        if !self.remote_enabled || !has_remote_management(capabilities) {
            return Ok(None);
        }

        self.k8s_store
            .get_opamp_data::<AgentRemoteConfigStatus>(
                agent_id,
                STORE_KEY_OPAMP_DATA_REMOTE_CONFIG_STATUS,
            )
            .map_err(|err| ConfigStatusManagerError::Retrieval(err.to_string()))
    }

    fn store_remote_status(
        &self,
        agent_id: &AgentID,
        status: &AgentRemoteConfigStatus,
    ) -> Result<(), ConfigStatusManagerError> {
        debug!(%agent_id, "saving remote config status");

        self.k8s_store
            .set_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_REMOTE_CONFIG_STATUS, status)
            .map_err(|err| ConfigStatusManagerError::Store(err.to_string()))
    }

    fn delete_remote_status(&self, agent_id: &AgentID) -> Result<(), ConfigStatusManagerError> {
        debug!(%agent_id, "deleting remote config status");

        self.k8s_store
            .delete_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_REMOTE_CONFIG_STATUS)
            .map_err(|err| ConfigStatusManagerError::Deletion(err.to_string()))
    }
}
