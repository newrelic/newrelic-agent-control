use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::{
    AgentControlConfig, AgentControlConfigError, AgentControlDynamicConfig,
};
use crate::agent_control::config_storer::loader_storer::{
    AgentControlConfigLoader, AgentControlDynamicConfigDeleter, AgentControlDynamicConfigLoader,
    AgentControlDynamicConfigStorer,
};
use crate::agent_control::defaults::{default_capabilities, AGENT_CONTROL_CONFIG_ENV_VAR_PREFIX};
use crate::values::yaml_config::{YAMLConfig, YAMLConfigError};
use crate::values::yaml_config_repository::{YAMLConfigRepository, YAMLConfigRepositoryError};
use config::builder::DefaultState;
use config::{Config, ConfigBuilder, Environment, File, FileFormat};
use opamp_client::operation::capabilities::Capabilities;
use std::sync::Arc;

pub struct AgentControlConfigStore<Y>
where
    Y: YAMLConfigRepository,
{
    config_builder: ConfigBuilder<DefaultState>,
    values_repository: Arc<Y>,
    agent_control_id: AgentID,
    agent_control_capabilities: Capabilities,
}

impl<Y> AgentControlConfigLoader for AgentControlConfigStore<Y>
where
    Y: YAMLConfigRepository,
{
    fn load(&self) -> Result<AgentControlConfig, AgentControlConfigError> {
        self._load_config()
    }
}

impl<Y> AgentControlDynamicConfigLoader for AgentControlConfigStore<Y>
where
    Y: YAMLConfigRepository,
{
    fn load(&self) -> Result<AgentControlDynamicConfig, AgentControlConfigError> {
        Ok(self._load_config()?.dynamic)
    }
}

impl<Y> AgentControlDynamicConfigDeleter for AgentControlConfigStore<Y>
where
    Y: YAMLConfigRepository,
{
    fn delete(&self) -> Result<(), AgentControlConfigError> {
        self.values_repository
            .delete_remote(&self.agent_control_id)?;
        Ok(())
    }
}

impl<Y> AgentControlDynamicConfigStorer for AgentControlConfigStore<Y>
where
    Y: YAMLConfigRepository,
{
    fn store(&self, yaml_config: &YAMLConfig) -> Result<(), AgentControlConfigError> {
        self.values_repository
            .store_remote(&self.agent_control_id, yaml_config)?;
        Ok(())
    }
}

impl<V> AgentControlConfigStore<V>
where
    V: YAMLConfigRepository,
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
            .try_into()
            .map_err(|e: YAMLConfigError| AgentControlConfigError::Load(e.to_string()))?;

        let mut config = self
            .config_builder
            .clone() // Pass default config file location and optionally, so we could pass all config through
            .add_source(File::from_str(
                local_config_string.as_str(),
                FileFormat::Yaml,
            ))
            // Add in settings from the environment (with a prefix of `NR_` and separator double underscore, `__`)
            // Eg.. `NR_LOG__USE_DEBUG=1 ./target/app` would set the `log.use_debug` key
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
            let dynamic_config: AgentControlDynamicConfig = remote_config.try_into()?;
            config.dynamic = dynamic_config;
        }

        Ok(config)
    }
}

impl From<YAMLConfigRepositoryError> for AgentControlConfigError {
    fn from(e: YAMLConfigRepositoryError) -> Self {
        match e {
            YAMLConfigRepositoryError::LoadError(e) => AgentControlConfigError::Load(e),
            YAMLConfigRepositoryError::StoreError(e) => AgentControlConfigError::Store(e),
            YAMLConfigRepositoryError::DeleteError(e) => AgentControlConfigError::Delete(e),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::agent_control::config::{
        AgentControlConfig, AgentTypeFQN, OpAMPClientConfig, SubAgentConfig,
    };
    use crate::agent_control::defaults::AGENT_CONTROL_CONFIG_FILENAME;
    use crate::values::file::YAMLConfigRepositoryFile;
    use serial_test::serial;
    use std::path::PathBuf;
    use std::{collections::HashMap, env};
    use url::Url;

    #[test]
    #[serial]
    fn load_agents_local_remote() {
        let local_dir = tempfile::tempdir().unwrap().into_path().to_path_buf();
        let local_file = local_dir.join(AGENT_CONTROL_CONFIG_FILENAME);
        let local_config = r#"
agents: {}
fleet_control:
  endpoint: http://127.0.0.1/v1/opamp
"#;
        std::fs::write(local_file.as_path(), local_config).unwrap();

        let remote_dir = tempfile::tempdir().unwrap().into_path().to_path_buf();
        let remote_file = remote_dir.join(AGENT_CONTROL_CONFIG_FILENAME);

        let remote_config = r#"
        agents:
          rolldice:
            agent_type: "namespace/com.newrelic.infrastructure:0.0.2"
        "#;
        std::fs::write(remote_file.as_path(), remote_config).unwrap();

        let vr = YAMLConfigRepositoryFile::new(local_dir, remote_dir).with_remote();
        let store = AgentControlConfigStore::new(Arc::new(vr));
        let actual = AgentControlConfigLoader::load(&store).unwrap();

        let expected = AgentControlConfig {
            dynamic: HashMap::from([(
                AgentID::new("rolldice").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::try_from(
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
        let local_dir = tempfile::tempdir().unwrap().into_path().to_path_buf();
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
        env::set_var(env_var_name, "namespace/com.newrelic.infrastructure:0.0.2");

        let vr = YAMLConfigRepositoryFile::new(local_dir, PathBuf::new()).with_remote();
        let store = AgentControlConfigStore::new(Arc::new(vr));
        let actual = AgentControlConfigLoader::load(&store).unwrap();

        let expected = AgentControlConfig {
            dynamic: HashMap::from([(
                AgentID::new("rolldice1").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::try_from(
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
        env::remove_var(env_var_name);

        assert_eq!(actual, expected);
    }

    #[test]
    #[serial]
    fn load_config_env_vars_override() {
        let local_dir = tempfile::tempdir().unwrap().into_path().to_path_buf();
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
        env::set_var(env_var_name, "namespace/com.newrelic.infrastructure:0.0.2");

        let vr = YAMLConfigRepositoryFile::new(local_dir, PathBuf::new()).with_remote();
        let store = AgentControlConfigStore::new(Arc::new(vr));
        let actual: AgentControlConfig = AgentControlConfigLoader::load(&store).unwrap();

        let expected = AgentControlConfig {
            dynamic: HashMap::from([(
                AgentID::new("rolldice2").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::try_from(
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
        env::remove_var(env_var_name);

        assert_eq!(actual, expected);
    }
}
