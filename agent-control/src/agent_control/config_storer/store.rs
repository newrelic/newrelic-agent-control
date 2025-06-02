use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::{
    AgentControlConfig, AgentControlConfigError, AgentControlDynamicConfig,
};
use crate::agent_control::config_storer::loader_storer::{
    AgentControlConfigLoader, AgentControlDynamicConfigLoader, AgentControlRemoteConfigDeleter,
    AgentControlRemoteConfigHashGetter, AgentControlRemoteConfigHashStateUpdater,
    AgentControlRemoteConfigStorer,
};
use crate::agent_control::defaults::{AGENT_CONTROL_CONFIG_ENV_VAR_PREFIX, default_capabilities};
use crate::opamp::remote_config::hash::{ConfigState, Hash};
use crate::values::config::RemoteConfig;
use crate::values::config_repository::{ConfigRepository, ConfigRepositoryError};
use crate::values::yaml_config::YAMLConfigError;
use config::builder::DefaultState;
use config::{Config, ConfigBuilder, Environment, File, FileFormat};
use opamp_client::operation::capabilities::Capabilities;
use std::sync::Arc;

pub struct AgentControlConfigStore<Y>
where
    Y: ConfigRepository,
{
    config_builder: ConfigBuilder<DefaultState>,
    values_repository: Arc<Y>,
    agent_control_id: AgentID,
    agent_control_capabilities: Capabilities,
}

impl<Y> AgentControlConfigLoader for AgentControlConfigStore<Y>
where
    Y: ConfigRepository,
{
    fn load(&self) -> Result<AgentControlConfig, AgentControlConfigError> {
        self._load_config()
    }
}

impl<Y> AgentControlDynamicConfigLoader for AgentControlConfigStore<Y>
where
    Y: ConfigRepository,
{
    fn load(&self) -> Result<AgentControlDynamicConfig, AgentControlConfigError> {
        Ok(self._load_config()?.dynamic)
    }
}

impl<Y> AgentControlRemoteConfigDeleter for AgentControlConfigStore<Y>
where
    Y: ConfigRepository,
{
    fn delete(&self) -> Result<(), AgentControlConfigError> {
        self.values_repository
            .delete_remote(&self.agent_control_id)?;
        Ok(())
    }
}

impl<Y> AgentControlRemoteConfigStorer for AgentControlConfigStore<Y>
where
    Y: ConfigRepository,
{
    fn store(&self, config: &RemoteConfig) -> Result<(), AgentControlConfigError> {
        self.values_repository
            .store_remote(&self.agent_control_id, config)?;
        Ok(())
    }
}

impl<Y> AgentControlRemoteConfigHashStateUpdater for AgentControlConfigStore<Y>
where
    Y: ConfigRepository,
{
    fn update_hash_state(&self, state: &ConfigState) -> Result<(), AgentControlConfigError> {
        self.values_repository
            .update_hash_state(&self.agent_control_id, state)?;
        Ok(())
    }
}

impl<Y> AgentControlRemoteConfigHashGetter for AgentControlConfigStore<Y>
where
    Y: ConfigRepository,
{
    fn get_hash(&self) -> Result<Option<Hash>, AgentControlConfigError> {
        Ok(self.values_repository.get_hash(&self.agent_control_id)?)
    }
}

impl<V> AgentControlConfigStore<V>
where
    V: ConfigRepository,
{
    pub fn new(values_repository: Arc<V>) -> Self {
        let config_builder = Config::builder();

        Self {
            config_builder,
            values_repository,
            agent_control_id: AgentID::new_agent_control_id(),
            agent_control_capabilities: default_capabilities(),
        }
    }

    /// Load configs from local and remote sources.
    /// From the remote config only the AgentControlDynamicConfig is retrieve and if available applied
    /// on top of the local config
    fn _load_config(&self) -> Result<AgentControlConfig, AgentControlConfigError> {
        let local_config_string: String = self
            .values_repository
            .load_local(&self.agent_control_id)?
            .ok_or(AgentControlConfigError::Load(
                "missing local agent control config".to_string(),
            ))?
            .get_yaml_config()
            .clone()
            .try_into()
            .map_err(|e: YAMLConfigError| AgentControlConfigError::Load(e.to_string()))?;

        let mut config = self
            .config_builder
            .clone() // Pass default config file location and optionally, so we could pass all config through
            .add_source(File::from_str(
                local_config_string.as_str(),
                FileFormat::Yaml,
            ))
            // Add in settings from the environment (with a prefix of `NR_AC_` and separator double underscore, `__`)
            // Eg.. `NR_AC_LOG__LEVEL=1 ./target/app` would set the `log.level` key to 1 (debug).
            // We use double underscore because we already use snake_case for the config keys.
            .add_source(
                Environment::with_prefix(AGENT_CONTROL_CONFIG_ENV_VAR_PREFIX)
                    .prefix_separator("_")
                    .separator("__"),
            )
            .build()?
            .try_deserialize::<AgentControlConfig>()?;

        if let Some(remote_config) = self
            .values_repository
            .load_remote(&self.agent_control_id, &self.agent_control_capabilities)?
        {
            let dynamic_config: AgentControlDynamicConfig =
                remote_config.get_yaml_config().clone().try_into()?;
            config.dynamic = dynamic_config;
        }

        Ok(config)
    }
}

impl From<ConfigRepositoryError> for AgentControlConfigError {
    fn from(e: ConfigRepositoryError) -> Self {
        match e {
            ConfigRepositoryError::LoadError(e) => AgentControlConfigError::Load(e),
            ConfigRepositoryError::StoreError(e) => AgentControlConfigError::Store(e),
            ConfigRepositoryError::DeleteError(e) => AgentControlConfigError::Delete(e),
            ConfigRepositoryError::UpdateHashStateError(e) => AgentControlConfigError::Update(e),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::agent_control::config::{AgentControlConfig, OpAMPClientConfig, SubAgentConfig};
    use crate::agent_control::defaults::AGENT_CONTROL_CONFIG_FILENAME;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::values::file::ConfigRepositoryFile;
    use serial_test::serial;
    use std::path::PathBuf;
    use std::{collections::HashMap, env};
    use url::Url;

    #[test]
    #[serial]
    fn load_agents_local_remote() {
        let local_temp_dir = tempfile::tempdir().unwrap();
        let local_dir = local_temp_dir.path().to_path_buf();
        let local_file = local_dir.join(AGENT_CONTROL_CONFIG_FILENAME);
        let local_config = r#"
agents: {}
fleet_control:
  endpoint: http://127.0.0.1/v1/opamp
"#;
        std::fs::write(local_file.as_path(), local_config).unwrap();

        let remote_temp_dir = tempfile::tempdir().unwrap();
        let remote_dir = remote_temp_dir.path().to_path_buf();
        let remote_file = remote_dir.join(AGENT_CONTROL_CONFIG_FILENAME);

        let remote_config = r#"
        config:
            agents:
              rolldice:
                agent_type: "namespace/com.newrelic.infrastructure:0.0.2"
        hash: a-hash
        state: applying
        "#;
        std::fs::write(remote_file.as_path(), remote_config).unwrap();

        let vr = ConfigRepositoryFile::new(local_dir, remote_dir).with_remote();
        let store = AgentControlConfigStore::new(Arc::new(vr));
        let actual = AgentControlConfigLoader::load(&store).unwrap();

        let expected = AgentControlConfig {
            dynamic: HashMap::from([(
                AgentID::new("rolldice").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeID::try_from(
                        "namespace/com.newrelic.infrastructure:0.0.2",
                    )
                    .unwrap(),
                },
            )])
            .into(),
            fleet_control: Some(OpAMPClientConfig {
                endpoint: Url::try_from("http://127.0.0.1/v1/opamp").unwrap(),
                ..Default::default()
            }),
            k8s: None,
            ..Default::default()
        };

        assert_eq!(actual, expected);
    }

    #[test]
    #[serial]
    fn load_config_env_vars() {
        let temp_dir = tempfile::tempdir().unwrap();
        let local_dir = temp_dir.path().to_path_buf();
        let local_file = local_dir.join(AGENT_CONTROL_CONFIG_FILENAME);

        // Note the file contains no `agents` key, which would fail if this config was the only
        // source checked when loading the local config.
        let local_config = r#"
agents: {}
fleet_control:
  endpoint: http://127.0.0.1/v1/opamp
"#;
        std::fs::write(local_file.as_path(), local_config).unwrap();

        // We set the environment variable with the `__` separator which will create the nested
        // configs appropriately.
        let env_var_name = "NR_AC_AGENTS__ROLLDICE1__AGENT_TYPE";
        unsafe { env::set_var(env_var_name, "namespace/com.newrelic.infrastructure:0.0.2") };

        let vr = ConfigRepositoryFile::new(local_dir, PathBuf::new()).with_remote();
        let store = AgentControlConfigStore::new(Arc::new(vr));
        let actual = AgentControlConfigLoader::load(&store).unwrap();

        let expected = AgentControlConfig {
            dynamic: HashMap::from([(
                AgentID::new("rolldice1").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeID::try_from(
                        "namespace/com.newrelic.infrastructure:0.0.2",
                    )
                    .unwrap(),
                },
            )])
            .into(),
            fleet_control: Some(OpAMPClientConfig {
                endpoint: Url::try_from("http://127.0.0.1/v1/opamp").unwrap(),
                ..Default::default()
            }),
            k8s: None,
            ..Default::default()
        };

        // Env cleanup
        unsafe { env::remove_var(env_var_name) };

        assert_eq!(actual, expected);
    }

    #[test]
    #[serial]
    fn load_config_env_vars_override() {
        let temp_dir = tempfile::tempdir().unwrap();
        let local_dir = temp_dir.path().to_path_buf();
        let local_file = local_dir.join(AGENT_CONTROL_CONFIG_FILENAME);
        let local_config = r#"
fleet_control:
  endpoint: http://127.0.0.1/v1/opamp
agents:
  rolldice2:
    agent_type: "namespace/will.be.overridden:0.0.1"
"#;
        std::fs::write(local_file.as_path(), local_config).unwrap();

        // We set the environment variable with the `__` separator which will create the nested
        // configs appropriately.
        let env_var_name = "NR_AC_AGENTS__ROLLDICE2__AGENT_TYPE";
        unsafe { env::set_var(env_var_name, "namespace/com.newrelic.infrastructure:0.0.2") };

        let vr = ConfigRepositoryFile::new(local_dir, PathBuf::new()).with_remote();
        let store = AgentControlConfigStore::new(Arc::new(vr));
        let actual: AgentControlConfig = AgentControlConfigLoader::load(&store).unwrap();

        let expected = AgentControlConfig {
            dynamic: HashMap::from([(
                AgentID::new("rolldice2").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeID::try_from(
                        "namespace/com.newrelic.infrastructure:0.0.2",
                    )
                    .unwrap(),
                },
            )])
            .into(),
            fleet_control: Some(OpAMPClientConfig {
                endpoint: Url::try_from("http://127.0.0.1/v1/opamp").unwrap(),
                ..Default::default()
            }),
            k8s: None,
            ..Default::default()
        };

        // Env cleanup
        unsafe { env::remove_var(env_var_name) };

        assert_eq!(actual, expected);
    }
}
