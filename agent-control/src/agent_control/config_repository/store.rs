use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::{
    AgentControlConfig, AgentControlConfigError, AgentControlDynamicConfig,
};
use crate::agent_control::config_repository::repository::{
    AgentControlConfigLoader, AgentControlDynamicConfigRepository,
};
use crate::agent_control::defaults::{AGENT_CONTROL_CONFIG_ENV_VAR_PREFIX, default_capabilities};
use crate::opamp::remote_config::hash::ConfigState;
use crate::values::config::RemoteConfig;
use crate::values::config_repository::{ConfigRepository, ConfigRepositoryError};
use crate::values::yaml_config::YAMLConfigError;
use config::builder::DefaultState;
use config::{Config, ConfigBuilder, Environment, File, FileFormat};
use opamp_client::operation::capabilities::Capabilities;
use std::sync::Arc;
use tracing::warn;

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

impl<Y> AgentControlDynamicConfigRepository for AgentControlConfigStore<Y>
where
    Y: ConfigRepository,
{
    fn load(&self) -> Result<AgentControlDynamicConfig, AgentControlConfigError> {
        Ok(self._load_config()?.dynamic)
    }

    fn store(&self, config: &RemoteConfig) -> Result<(), AgentControlConfigError> {
        self.values_repository
            .store_remote(&self.agent_control_id, config)?;
        Ok(())
    }

    fn update_state(&self, state: ConfigState) -> Result<(), AgentControlConfigError> {
        self.values_repository
            .update_state(&self.agent_control_id, state)?;
        Ok(())
    }

    fn delete(&self) -> Result<(), AgentControlConfigError> {
        self.values_repository
            .delete_remote(&self.agent_control_id)?;
        Ok(())
    }

    fn get_remote_config(&self) -> Result<Option<RemoteConfig>, AgentControlConfigError> {
        Ok(self
            .values_repository
            .get_remote_config(&self.agent_control_id)?)
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
            agent_control_id: AgentID::AgentControl,
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

        config.dynamic = sanitize_local_dynamic_config(config.dynamic);

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

/// Removes an configuration from local values that is only supported on remote configs.
fn sanitize_local_dynamic_config(
    dynamic_config: AgentControlDynamicConfig,
) -> AgentControlDynamicConfig {
    // Remove any AC update config. This prevents to downgrade the AC in case of a remote config reset.
    // It also prevents that any local miss-configuration change the local version deployed.
    if let Some(chart_version) = dynamic_config.chart_version {
        warn!(
            "The 'chart_version' value: `{}` was found in the local configuration but is not supported and will be ignored",
            chart_version
        );
    }
    AgentControlDynamicConfig {
        chart_version: None,
        ..dynamic_config
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
    use crate::agent_control::agent_id::{AgentID, SubAgentID};
    use crate::agent_control::config::{
        AgentControlConfig, AgentControlDynamicConfig, SubAgentConfig,
    };
    use crate::agent_control::config_repository::repository::AgentControlConfigLoader;
    use crate::agent_control::config_repository::store::AgentControlConfigStore;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::values::config_repository::ConfigRepository;
    use crate::values::config_repository::tests::InMemoryConfigRepository;
    use crate::values::yaml_config::YAMLConfig;
    use serial_test::{parallel, serial};
    use std::collections::HashMap;
    use std::env;
    use std::sync::Arc;

    #[test]
    #[parallel]
    fn load_local_config_with_empty_remote() {
        let config_repository = InMemoryConfigRepository::default();

        let local_config = r#"
        agents: {}
        host_id: some
        "#
        .try_into()
        .unwrap();

        config_repository
            .store_local(&AgentID::AgentControl, &local_config)
            .unwrap();

        let store = AgentControlConfigStore::new(Arc::new(config_repository));
        let actual = AgentControlConfigLoader::load(&store).unwrap();

        let expected = AgentControlConfig {
            host_id: "some".to_string(),
            ..Default::default()
        };
        assert_eq!(actual, expected);
    }

    #[test]
    #[parallel]
    fn load_remote_overrides_dynamic_values_from_local() {
        let config_repository = InMemoryConfigRepository::default();

        let local_config = r#"
        agents: {}
        host_id: some
        "#
        .try_into()
        .unwrap();

        config_repository
            .store_local(&AgentID::AgentControl, &local_config)
            .unwrap();
        let remote_config: YAMLConfig = r#"
        agents:
          rolldice:
            agent_type: "namespace/name:0.0.2"
        chart_version: "1.0.0"
        "#
        .try_into()
        .unwrap();

        config_repository
            .store_remote(&AgentID::AgentControl, &remote_config.clone().into())
            .unwrap();

        let store = AgentControlConfigStore::new(Arc::new(config_repository));
        let actual = AgentControlConfigLoader::load(&store).unwrap();

        let expected = AgentControlConfig {
            dynamic: AgentControlDynamicConfig {
                agents: HashMap::from([(
                    SubAgentID::try_from("rolldice").unwrap(),
                    SubAgentConfig {
                        agent_type: AgentTypeID::try_from("namespace/name:0.0.2").unwrap(),
                    },
                )]),
                chart_version: Some("1.0.0".to_string()),
                cd_chart_version: None,
            },
            host_id: "some".to_string(),
            ..Default::default()
        };
        assert_eq!(actual, expected)
    }

    #[test]
    #[serial]
    fn load_config_env_vars() {
        let config_repository = InMemoryConfigRepository::default();

        let local_config = r#"
        agents: {}
        host_id: some
        "#
        .try_into()
        .unwrap();

        config_repository
            .store_local(&AgentID::AgentControl, &local_config)
            .unwrap();

        // We set the environment variable with the `__` separator which will create the nested
        // configs appropriately.
        let env_var_name = "NR_AC_AGENTS__ROLLDICE__AGENT_TYPE";
        unsafe { env::set_var(env_var_name, "namespace/name:0.0.2") };

        let store = AgentControlConfigStore::new(Arc::new(config_repository));
        let actual = AgentControlConfigLoader::load(&store).unwrap();

        let expected = AgentControlConfig {
            dynamic: AgentControlDynamicConfig {
                agents: HashMap::from([(
                    SubAgentID::try_from("rolldice").unwrap(),
                    SubAgentConfig {
                        agent_type: AgentTypeID::try_from("namespace/name:0.0.2").unwrap(),
                    },
                )]),
                ..Default::default()
            },
            host_id: "some".to_string(),
            ..Default::default()
        };

        // Env cleanup
        unsafe { env::remove_var(env_var_name) };

        assert_eq!(actual, expected);
    }

    #[test]
    #[serial]
    fn load_config_env_vars_have_precedence() {
        let config_repository = InMemoryConfigRepository::default();

        let local_config = r#"
        agents:
          overrideme:
            agent_type: "namespace/overrideme:0.0.1"
        "#
        .try_into()
        .unwrap();

        config_repository
            .store_local(&AgentID::AgentControl, &local_config)
            .unwrap();

        // We set the environment variable with the `__` separator which will create the nested
        // configs appropriately.
        let env_var_name = "NR_AC_AGENTS__OVERRIDEME__AGENT_TYPE";
        unsafe { env::set_var(env_var_name, "namespace/from.env:0.0.2") };

        let store = AgentControlConfigStore::new(Arc::new(config_repository));
        let actual = AgentControlConfigLoader::load(&store).unwrap();

        let expected = AgentControlConfig {
            dynamic: AgentControlDynamicConfig {
                agents: HashMap::from([(
                    SubAgentID::try_from("overrideme").unwrap(),
                    SubAgentConfig {
                        agent_type: AgentTypeID::try_from("namespace/from.env:0.0.2").unwrap(),
                    },
                )]),
                ..Default::default()
            },
            ..Default::default()
        };

        // Env cleanup
        unsafe { env::remove_var(env_var_name) };

        assert_eq!(actual, expected);
    }

    #[test]
    #[parallel]
    fn load_local_sanitized_values() {
        let config_repository = InMemoryConfigRepository::default();

        let local_config = r#"
        agents: {}
        host_id: some
        chart_values: "1.0.0"
        "#
        .try_into()
        .unwrap();

        config_repository
            .store_local(&AgentID::AgentControl, &local_config)
            .unwrap();

        let store = AgentControlConfigStore::new(Arc::new(config_repository));
        let actual = AgentControlConfigLoader::load(&store).unwrap();

        let expected = AgentControlConfig {
            host_id: "some".to_string(),
            //chart_values gets removed
            ..Default::default()
        };
        assert_eq!(actual, expected);
    }
}
