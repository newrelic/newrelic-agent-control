use crate::k8s::store::{K8sStore, STORE_KEY_OPAMP_DATA_CONFIG};
use crate::super_agent::config::{AgentID, SubAgentsConfig, SuperAgentConfigError};
use crate::super_agent::config_storer::storer::{
    SubAgentsConfigDeleter, SubAgentsConfigLoader, SubAgentsConfigStorer,
};
use std::sync::Arc;
use tracing::debug;

pub struct SubAgentListStorerConfigMap {
    k8s_store: Arc<K8sStore>,
    remote_enabled: bool,
    super_agent_id: AgentID,
    local_config_cached: SubAgentsConfig,
}

impl SubAgentsConfigLoader for SubAgentListStorerConfigMap {
    fn load(&self) -> Result<SubAgentsConfig, SuperAgentConfigError> {
        debug!(
            super_agent_id = self.super_agent_id.to_string(),
            "loading local config of subagent list"
        );

        if self.remote_enabled {
            if let Some(remote_subagent_list_config) =
                self.k8s_store.get_opamp_data::<SubAgentsConfig>(
                    &self.super_agent_id,
                    STORE_KEY_OPAMP_DATA_CONFIG,
                )?
            {
                debug!(
                    super_agent_id = self.super_agent_id.to_string(),
                    "updating the list of subAgents with the one received from opamp"
                );
                return Ok(remote_subagent_list_config);
            }
        }

        Ok(self.local_config_cached.clone())
    }
}

impl SubAgentsConfigDeleter for SubAgentListStorerConfigMap {
    fn delete(&self) -> Result<(), SuperAgentConfigError> {
        debug!(
            super_agent_id = self.super_agent_id.to_string(),
            "deleting remote config of subagent list"
        );

        self.k8s_store
            .delete_opamp_data(&self.super_agent_id, STORE_KEY_OPAMP_DATA_CONFIG)?;
        Ok(())
    }
}

impl SubAgentsConfigStorer for SubAgentListStorerConfigMap {
    fn store(&self, sub_agents: &SubAgentsConfig) -> Result<(), SuperAgentConfigError> {
        debug!(
            super_agent_id = self.super_agent_id.to_string(),
            "saving remote config of subagent list"
        );

        self.k8s_store.set_opamp_data(
            &self.super_agent_id,
            STORE_KEY_OPAMP_DATA_CONFIG,
            sub_agents,
        )?;
        Ok(())
    }
}

impl SubAgentListStorerConfigMap {
    pub fn new(k8s_store: Arc<K8sStore>, local_config_cached: SubAgentsConfig) -> Self {
        Self {
            super_agent_id: AgentID::new_super_agent_id(),
            k8s_store,
            remote_enabled: false,
            local_config_cached,
        }
    }

    pub fn with_remote(self) -> Self {
        Self {
            remote_enabled: true,
            super_agent_id: self.super_agent_id,
            k8s_store: self.k8s_store.clone(),
            local_config_cached: self.local_config_cached,
        }
    }
}
