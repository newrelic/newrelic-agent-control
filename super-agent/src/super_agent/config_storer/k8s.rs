use crate::k8s::store::{K8sStore, STORE_KEY_OPAMP_DATA_CONFIG};
use crate::super_agent::config::{AgentID, SuperAgentConfigError, SuperAgentDynamicConfig};
use crate::super_agent::config_storer::loader_storer::{
    SuperAgentDynamicConfigDeleter, SuperAgentDynamicConfigLoader, SuperAgentDynamicConfigStorer,
};
use std::sync::Arc;
use tracing::debug;

pub struct SubAgentsConfigStoreConfigMap {
    k8s_store: Arc<K8sStore>,
    remote_enabled: bool,
    super_agent_id: AgentID,
    local_config: SuperAgentDynamicConfig,
}

impl SuperAgentDynamicConfigLoader for SubAgentsConfigStoreConfigMap {
    fn load(&self) -> Result<SuperAgentDynamicConfig, SuperAgentConfigError> {
        if self.remote_enabled {
            if let Some(remote_subagent_config) =
                self.k8s_store.get_opamp_data::<SuperAgentDynamicConfig>(
                    &self.super_agent_id,
                    STORE_KEY_OPAMP_DATA_CONFIG,
                )?
            {
                debug!("loading remote SuperAgentDynamicConfig");
                return Ok(remote_subagent_config);
            }
        }

        debug!(
            super_agent_id = self.super_agent_id.to_string(),
            "loading local SuperAgentDynamicConfig"
        );
        Ok(self.local_config.clone())
    }
}

impl SuperAgentDynamicConfigDeleter for SubAgentsConfigStoreConfigMap {
    fn delete_remote(&self) -> Result<(), SuperAgentConfigError> {
        debug!("deleting remote SuperAgentDynamicConfig");

        self.k8s_store
            .delete_opamp_data(&self.super_agent_id, STORE_KEY_OPAMP_DATA_CONFIG)?;
        Ok(())
    }
}

impl SuperAgentDynamicConfigStorer for SubAgentsConfigStoreConfigMap {
    fn store_remote(
        &self,
        sub_agents: &SuperAgentDynamicConfig,
    ) -> Result<(), SuperAgentConfigError> {
        debug!("saving remote SuperAgentDynamicConfig");

        self.k8s_store.set_opamp_data(
            &self.super_agent_id,
            STORE_KEY_OPAMP_DATA_CONFIG,
            sub_agents,
        )?;
        Ok(())
    }
}

impl SubAgentsConfigStoreConfigMap {
    pub fn new(k8s_store: Arc<K8sStore>, local_config_cached: SuperAgentDynamicConfig) -> Self {
        Self {
            super_agent_id: AgentID::new_super_agent_id(),
            k8s_store,
            remote_enabled: false,
            local_config: local_config_cached,
        }
    }

    pub fn with_remote(self) -> Self {
        Self {
            remote_enabled: true,
            ..self
        }
    }
}
