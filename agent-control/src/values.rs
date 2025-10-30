use std::sync::Arc;

use opamp_client::operation::capabilities::Capabilities;
use tracing::debug;

use crate::{
    agent_control::{
        agent_id::AgentID,
        defaults::{STORE_KEY_LOCAL_DATA_CONFIG, STORE_KEY_OPAMP_DATA_CONFIG},
    },
    opamp::{data_store::OpAMPDataStore, remote_config::hash::ConfigState},
    values::{
        config::{Config, RemoteConfig},
        config_repository::{ConfigRepository, ConfigRepositoryError},
        yaml_config::{YAMLConfig, has_remote_management},
    },
};

pub mod config;
pub mod config_repository;
pub mod yaml_config;

pub struct ConfigRepo<D: OpAMPDataStore> {
    opamp_data_store: Arc<D>,
    remote_enabled: bool,
}

impl<D: OpAMPDataStore> ConfigRepo<D> {
    pub fn new(opamp_data_store: Arc<D>) -> Self {
        Self {
            opamp_data_store,
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

impl<D> ConfigRepository for ConfigRepo<D>
where
    D: OpAMPDataStore + Send + Sync + 'static,
{
    #[tracing::instrument(skip_all, err)]
    fn load_local(&self, agent_id: &AgentID) -> Result<Option<Config>, ConfigRepositoryError> {
        self.opamp_data_store
            .get_local_data::<YAMLConfig>(agent_id, STORE_KEY_LOCAL_DATA_CONFIG)
            .map_err(|err| ConfigRepositoryError::LoadError(format!("loading local config: {err}")))
            .map(|opt_yaml| opt_yaml.map(|yc| Config::LocalConfig(yc.into())))
    }

    #[tracing::instrument(skip_all, err)]
    fn load_remote(
        &self,
        agent_id: &AgentID,
        capabilities: &Capabilities,
    ) -> Result<Option<Config>, ConfigRepositoryError> {
        if !self.remote_enabled || !has_remote_management(capabilities) {
            Ok(None)
        } else {
            self.opamp_data_store
                .get_opamp_data::<RemoteConfig>(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
                .map_err(|err| {
                    ConfigRepositoryError::LoadError(format!("loading remote config: {err}"))
                })
                .map(|opt_rc| opt_rc.map(Config::RemoteConfig))
        }
    }

    #[tracing::instrument(skip_all, err)]
    fn store_remote(
        &self,
        agent_id: &AgentID,
        remote_config: &RemoteConfig,
    ) -> Result<(), ConfigRepositoryError> {
        debug!(agent_id = agent_id.to_string(), "saving remote config");

        self.opamp_data_store
            .set_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_CONFIG, remote_config)
            .map_err(|e| ConfigRepositoryError::StoreError(format!("storing remote config: {}", e)))
    }

    fn get_remote_config(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<RemoteConfig>, ConfigRepositoryError> {
        self.opamp_data_store
            .get_opamp_data::<RemoteConfig>(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|e| {
                ConfigRepositoryError::LoadError(format!("getting remote config hash: {}", e))
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
            .opamp_data_store
            .get_opamp_data::<RemoteConfig>(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|e| {
                ConfigRepositoryError::LoadError(format!("updating remote config state: {e}"))
            })?;

        match maybe_config {
            Some(remote_config) => self
                .opamp_data_store
                .set_opamp_data(
                    agent_id,
                    STORE_KEY_OPAMP_DATA_CONFIG,
                    &remote_config.with_state(state),
                )
                .map_err(|err| {
                    ConfigRepositoryError::StoreError(format!(
                        "updating remote config state: {err}"
                    ))
                }),
            None => Err(ConfigRepositoryError::UpdateHashStateError(
                "No remote config found".to_string(),
            )),
        }
    }

    // TODO Currently we are not deleting the whole folder, therefore multiple files are not supported
    // Moreover, we are also loading one file only, therefore we should review this once support is added
    // Notice that in that case we will likely need to move AgentControlConfig file to a folder
    #[tracing::instrument(skip_all err)]
    fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ConfigRepositoryError> {
        debug!(agent_id = agent_id.to_string(), "deleting remote config");

        self.opamp_data_store
            .delete_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_CONFIG)
            .map_err(|e| {
                ConfigRepositoryError::DeleteError(format!("deleting remote config: {}", e))
            })
    }
}
