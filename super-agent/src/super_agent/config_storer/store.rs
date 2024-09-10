use crate::super_agent::config::{
    AgentID, SuperAgentConfig, SuperAgentConfigError, SuperAgentDynamicConfig,
};
use crate::super_agent::config_storer::loader_storer::{
    SuperAgentConfigLoader, SuperAgentDynamicConfigDeleter, SuperAgentDynamicConfigLoader,
    SuperAgentDynamicConfigStorer,
};
use crate::super_agent::defaults::{default_capabilities, SUPER_AGENT_CONFIG_ENV_VAR_PREFIX};
use crate::values::yaml_config::{YAMLConfig, YAMLConfigError};
use crate::values::yaml_config_repository::{YAMLConfigRepository, YAMLConfigRepositoryError};
use config::builder::DefaultState;
use config::{Config, ConfigBuilder, Environment, File, FileFormat};
use opamp_client::operation::capabilities::Capabilities;
use std::sync::Arc;

pub struct SuperAgentConfigStore<Y>
where
    Y: YAMLConfigRepository,
{
    config_builder: ConfigBuilder<DefaultState>,
    values_repository: Arc<Y>,
    super_agent_id: AgentID,
    super_agent_capabilities: Capabilities,
}

impl<Y> SuperAgentConfigLoader for SuperAgentConfigStore<Y>
where
    Y: YAMLConfigRepository,
{
    fn load(&self) -> Result<SuperAgentConfig, SuperAgentConfigError> {
        self._load_config()
    }
}

impl<Y> SuperAgentDynamicConfigLoader for SuperAgentConfigStore<Y>
where
    Y: YAMLConfigRepository,
{
    fn load(&self) -> Result<SuperAgentDynamicConfig, SuperAgentConfigError> {
        Ok(self._load_config()?.dynamic)
    }
}

impl<Y> SuperAgentDynamicConfigDeleter for SuperAgentConfigStore<Y>
where
    Y: YAMLConfigRepository,
{
    fn delete(&self) -> Result<(), SuperAgentConfigError> {
        self.values_repository.delete_remote(&self.super_agent_id)?;
        Ok(())
    }
}

impl<Y> SuperAgentDynamicConfigStorer for SuperAgentConfigStore<Y>
where
    Y: YAMLConfigRepository,
{
    fn store(&self, yaml_config: &YAMLConfig) -> Result<(), SuperAgentConfigError> {
        self.values_repository
            .store_remote(&self.super_agent_id, yaml_config)?;
        Ok(())
    }
}

impl<V> SuperAgentConfigStore<V>
where
    V: YAMLConfigRepository,
{
    pub fn new(values_repository: Arc<V>) -> Self {
        let config_builder = Config::builder();

        Self {
            config_builder,
            values_repository,
            super_agent_id: AgentID::new_super_agent_id(),
            super_agent_capabilities: default_capabilities(),
        }
    }

    /// Load configs from local and remote sources.
    /// From the remote config only the SuperAgentDynamicConfig is retrieve and if available applied
    /// on top of the local config
    fn _load_config(&self) -> Result<SuperAgentConfig, SuperAgentConfigError> {
        let local_config_string: String = self
            .values_repository
            .load_local(&self.super_agent_id)?
            .ok_or(SuperAgentConfigError::Load(
                "missing local super agent config".to_string(),
            ))?
            .try_into()
            .map_err(|e: YAMLConfigError| SuperAgentConfigError::Load(e.to_string()))?;

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
                Environment::with_prefix(SUPER_AGENT_CONFIG_ENV_VAR_PREFIX)
                    .prefix_separator("_")
                    .separator("__"),
            )
            .build()?
            .try_deserialize::<SuperAgentConfig>()?;

        if let Some(remote_config) = self
            .values_repository
            .load_remote(&self.super_agent_id, &self.super_agent_capabilities)?
        {
            let dynamic_config: SuperAgentDynamicConfig = remote_config.try_into()?;
            config.dynamic = dynamic_config;
        }

        Ok(config)
    }
}

impl From<YAMLConfigRepositoryError> for SuperAgentConfigError {
    fn from(e: YAMLConfigRepositoryError) -> Self {
        match e {
            YAMLConfigRepositoryError::LoadError(e) => SuperAgentConfigError::Load(e),
            YAMLConfigRepositoryError::StoreError(e) => SuperAgentConfigError::Store(e),
            YAMLConfigRepositoryError::DeleteError(e) => SuperAgentConfigError::Delete(e),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::super_agent::config::{
        AgentID, AgentTypeFQN, OpAMPClientConfig, SubAgentConfig, SuperAgentConfig,
    };
    use crate::super_agent::defaults::SUPER_AGENT_CONFIG_FILE;
    use crate::values::file::YAMLConfigRepositoryFile;
    use serial_test::serial;
    use std::path::PathBuf;
    use std::{collections::HashMap, env};
    use url::Url;

    #[test]
    #[serial]
    fn load_agents_local_remote() {
        let local_dir = tempfile::tempdir().unwrap().into_path().to_path_buf();
        let local_file = local_dir.join(SUPER_AGENT_CONFIG_FILE);
        let local_config = r#"
agents: {}
opamp:
  endpoint: http://127.0.0.1/v1/opamp
"#;
        std::fs::write(local_file.as_path(), local_config).unwrap();

        let remote_dir = tempfile::tempdir().unwrap().into_path().to_path_buf();
        let remote_file = remote_dir.join(SUPER_AGENT_CONFIG_FILE);

        let remote_config = r#"
        agents:
          rolldice:
            agent_type: "namespace/com.newrelic.infrastructure_agent:0.0.2"
        "#;
        std::fs::write(remote_file.as_path(), remote_config).unwrap();

        let vr = YAMLConfigRepositoryFile::new(local_dir, remote_dir).with_remote();
        let store = SuperAgentConfigStore::new(Arc::new(vr));
        let actual = SuperAgentConfigLoader::load(&store).unwrap();

        let expected = SuperAgentConfig {
            dynamic: HashMap::from([(
                AgentID::new("rolldice").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::try_from(
                        "namespace/com.newrelic.infrastructure_agent:0.0.2",
                    )
                    .unwrap(),
                },
            )])
            .into(),
            opamp: Some(OpAMPClientConfig {
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
        let local_file = local_dir.join(SUPER_AGENT_CONFIG_FILE);

        // Note the file contains no `agents` key, which would fail if this config was the only
        // source checked when loading the local config.
        let local_config = r#"
agents: {}
opamp:
  endpoint: http://127.0.0.1/v1/opamp
"#;
        std::fs::write(local_file.as_path(), local_config).unwrap();

        // We set the environment variable with the `__` separator which will create the nested
        // configs appropriately.
        let env_var_name = "NR_SA_AGENTS__ROLLDICE1__AGENT_TYPE";
        env::set_var(
            env_var_name,
            "namespace/com.newrelic.infrastructure_agent:0.0.2",
        );

        let vr = YAMLConfigRepositoryFile::new(local_dir, PathBuf::new()).with_remote();
        let store = SuperAgentConfigStore::new(Arc::new(vr));
        let actual = SuperAgentConfigLoader::load(&store).unwrap();

        let expected = SuperAgentConfig {
            dynamic: HashMap::from([(
                AgentID::new("rolldice1").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::try_from(
                        "namespace/com.newrelic.infrastructure_agent:0.0.2",
                    )
                    .unwrap(),
                },
            )])
            .into(),
            opamp: Some(OpAMPClientConfig {
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
        let local_file = local_dir.join(SUPER_AGENT_CONFIG_FILE);
        let local_config = r#"
opamp:
  endpoint: http://127.0.0.1/v1/opamp
agents:
  rolldice2:
    agent_type: "namespace/will.be.overridden:0.0.1"
"#;
        std::fs::write(local_file.as_path(), local_config).unwrap();

        // We set the environment variable with the `__` separator which will create the nested
        // configs appropriately.
        let env_var_name = "NR_SA_AGENTS__ROLLDICE2__AGENT_TYPE";
        env::set_var(
            env_var_name,
            "namespace/com.newrelic.infrastructure_agent:0.0.2",
        );

        let vr = YAMLConfigRepositoryFile::new(local_dir, PathBuf::new()).with_remote();
        let store = SuperAgentConfigStore::new(Arc::new(vr));
        let actual: SuperAgentConfig = SuperAgentConfigLoader::load(&store).unwrap();

        let expected = SuperAgentConfig {
            dynamic: HashMap::from([(
                AgentID::new("rolldice2").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::try_from(
                        "namespace/com.newrelic.infrastructure_agent:0.0.2",
                    )
                    .unwrap(),
                },
            )])
            .into(),
            opamp: Some(OpAMPClientConfig {
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
