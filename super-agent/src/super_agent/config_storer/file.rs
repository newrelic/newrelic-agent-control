use crate::super_agent::config::{
    AgentID, SuperAgentConfig, SuperAgentConfigError, SuperAgentDynamicConfig,
};
use crate::super_agent::config_storer::loader_storer::{
    SuperAgentConfigLoader, SuperAgentDynamicConfigDeleter, SuperAgentDynamicConfigLoader,
    SuperAgentDynamicConfigStorer,
};
use crate::values::values_repository::ValuesRepository;
use crate::values::yaml_config::YAMLConfig;
use config::builder::DefaultState;
use config::{Config, ConfigBuilder, Environment};
use std::sync::Arc;

#[derive(thiserror::Error, Debug)]
pub enum ConfigStoreError {
    #[error("loading config: `{0}`")]
    IOError(#[from] std::io::Error),
    #[error("loading config: `{0}`")]
    SerdeYamlError(#[from] serde_yaml::Error),
    #[error("retrieving config: `{0}`")]
    ConfigError(#[from] config::ConfigError),
    #[error("deleting remote config: `{0}`")]
    Delete(String),
    #[error("loading config: `{0}`")]
    Load(String),
    #[error("storing config: `{0}`")]
    Store(String),
}

pub struct SuperAgentConfigStore<V>
where
    V: ValuesRepository,
{
    config_builder: ConfigBuilder<DefaultState>,
    values_repository: Arc<V>,
}

impl<V> SuperAgentConfigLoader for SuperAgentConfigStore<V>
where
    V: ValuesRepository,
{
    fn load(&self) -> Result<SuperAgentConfig, SuperAgentConfigError> {
        Ok(self._load_config()?) //wrapper to encapsulate error
    }
}

impl<V> SuperAgentDynamicConfigLoader for SuperAgentConfigStore<V>
where
    V: ValuesRepository,
{
    fn load(&self) -> Result<SuperAgentDynamicConfig, SuperAgentConfigError> {
        Ok(self._load_config()?.dynamic)
    }
}

impl<V> SuperAgentDynamicConfigDeleter for SuperAgentConfigStore<V>
where
    V: ValuesRepository,
{
    fn delete_remote(&self) -> Result<(), SuperAgentConfigError> {
        self.values_repository
            .delete_remote(&AgentID::new_super_agent_id())
            .map_err(|e| ConfigStoreError::Delete(e.to_string()))?;
        Ok(())
    }
}

impl<V> SuperAgentDynamicConfigStorer for SuperAgentConfigStore<V>
where
    V: ValuesRepository,
{
    fn store_remote(
        &self,
        sub_agents: &SuperAgentDynamicConfig,
    ) -> Result<(), SuperAgentConfigError> {
        self.values_repository
            .store_remote(
                &AgentID::new_super_agent_id(),
                &YAMLConfig::try_from(sub_agents)?,
            )
            .map_err(|e| ConfigStoreError::Store(e.to_string()))?;
        Ok(())
    }
}

impl<V> SuperAgentConfigStore<V>
where
    V: ValuesRepository,
{
    pub fn new(values_repository: Arc<V>) -> Self {
        let config_builder = Config::builder()
            // Add in settings from the environment (with a prefix of `NR_` and separator double underscore, `__`)
            // Eg.. `NR_LOG__USE_DEBUG=1 ./target/app` would set the `log.use_debug` key
            // We use double underscore because we already use snake_case for the config keys. TODO to change?
            .add_source(
                Environment::with_prefix("NR_SA")
                    .prefix_separator("_")
                    .separator("__"),
            );

        Self {
            values_repository,
            config_builder,
        }
    }

    /// Load leverages the value_repository to retrieve the YAML config and the config_builder to inject the
    /// environment variables.
    fn _load_config(&self) -> Result<SuperAgentConfig, ConfigStoreError> {
        let yaml_config = self
            .values_repository
            .load(&AgentID::new_super_agent_id())
            .map_err(|e| ConfigStoreError::Load(e.to_string()))?;

        let local_config = self
            .config_builder
            .clone() // Pass default config file location and optionally, so we could pass all config through
            .add_source(Config::try_from(&SuperAgentConfig::try_from(
                &yaml_config,
            )?)?)
            .build()?
            // From the retrieved config, attempt to generate the `SuperAgentConfig`
            .try_deserialize::<SuperAgentConfig>()?;

        Ok(local_config)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::super_agent::config::{
        AgentID, AgentTypeFQN, OpAMPClientConfig, SubAgentConfig, SuperAgentConfig,
    };
    use serial_test::serial;
    use std::{collections::HashMap, env, io::Write};
    use tempfile::NamedTempFile;
    use url::Url;

    #[test]
    fn load_agents_local_remote() {
        let mut local_file = NamedTempFile::new().unwrap();
        let local_config = r#"
agents: {}
opamp:
  endpoint: http://127.0.0.1/v1/opamp
"#;
        write!(local_file, "{}", local_config).unwrap();

        let mut remote_file = NamedTempFile::new().unwrap();

        let remote_config = r#"
agents:
  rolldice:
    agent_type: "namespace/com.newrelic.infrastructure_agent:0.0.2"
"#;
        write!(remote_file, "{}", remote_config).unwrap();

        let mut store = SuperAgentConfigStore::new(local_file.path());

        store.remote_path = Some(remote_file.path().to_path_buf());

        let actual = SuperAgentConfigLoader::load(&store);

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

        assert!(actual.is_ok());
        assert_eq!(actual.unwrap(), expected);
    }

    #[test]
    #[serial]
    fn load_config_env_vars() {
        let mut local_file = NamedTempFile::new().unwrap();
        // Note the file contains no `agents` key, which would fail if this config was the only
        // source checked when loading the local config.
        let local_config = r#"
opamp:
  endpoint: http://127.0.0.1/v1/opamp
"#;
        write!(local_file, "{}", local_config).unwrap();

        // We set the environment variable with the `__` separator which will create the nested
        // configs appropriately.
        let env_var_name = "NR_SA_AGENTS__ROLLDICE1__AGENT_TYPE";
        env::set_var(
            env_var_name,
            "namespace/com.newrelic.infrastructure_agent:0.0.2",
        );

        let store = SuperAgentConfigStore::new(local_file.path());
        let actual = SuperAgentConfigLoader::load(&store);

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

        assert!(actual.is_ok());
        assert_eq!(actual.unwrap(), expected);
    }

    #[test]
    #[serial]
    fn load_config_env_vars_override() {
        let mut local_file = NamedTempFile::new().unwrap();
        let local_config = r#"
opamp:
  endpoint: http://127.0.0.1/v1/opamp
agents:
  rolldice2:
    agent_type: "namespace/will.be.overridden:0.0.1"
"#;
        write!(local_file, "{}", local_config).unwrap();

        // We set the environment variable with the `__` separator which will create the nested
        // configs appropriately.
        let env_var_name = "NR_SA_AGENTS__ROLLDICE2__AGENT_TYPE";
        env::set_var(
            env_var_name,
            "namespace/com.newrelic.infrastructure_agent:0.0.2",
        );

        let store = SuperAgentConfigStore::new(local_file.path());
        let actual = SuperAgentConfigLoader::load(&store);

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

        assert!(actual.is_ok());
        assert_eq!(actual.unwrap(), expected);
    }
}
