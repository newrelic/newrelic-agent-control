use crate::super_agent::config::{
    SuperAgentConfig, SuperAgentConfigError, SuperAgentDynamicConfig,
};
use crate::super_agent::config_storer::loader_storer::{
    SuperAgentConfigLoader, SuperAgentDynamicConfigDeleter, SuperAgentDynamicConfigLoader,
    SuperAgentDynamicConfigStorer,
};
use config::builder::DefaultState;
use config::{Config, ConfigBuilder, Environment, File, FileFormat};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tracing::warn;

#[derive(thiserror::Error, Debug)]
pub enum ConfigStoreError {
    #[error("error loading config: `{0}`")]
    IOError(#[from] std::io::Error),

    #[error("error loading config: `{0}`")]
    SerdeYamlError(#[from] serde_yaml::Error),

    #[error("error retrieving config: `{0}`")]
    ConfigError(#[from] config::ConfigError),
}

pub struct SuperAgentConfigStore {
    local_path: PathBuf,
    remote_path: Option<PathBuf>,
    config_builder: ConfigBuilder<DefaultState>,
    rw_lock: RwLock<()>,
}

impl SuperAgentConfigLoader for SuperAgentConfigStore {
    fn load(&self) -> Result<SuperAgentConfig, SuperAgentConfigError> {
        Ok(self._load_config()?) //wrapper to encapsulate error
    }
}

impl SuperAgentDynamicConfigLoader for SuperAgentConfigStore {
    fn load(&self) -> Result<SuperAgentDynamicConfig, SuperAgentConfigError> {
        Ok(self._load_config()?.dynamic)
    }
}

impl SuperAgentDynamicConfigDeleter for SuperAgentConfigStore {
    //TODO this code is not unit tested
    fn delete(&self) -> Result<(), SuperAgentConfigError> {
        let Some(remote_path_file) = &self.remote_path else {
            unreachable!("we should not write into local paths");
        };
        let _write_guard = self.rw_lock.write().unwrap();
        if remote_path_file.exists() {
            std::fs::remove_file(remote_path_file)?;
        }
        Ok(())
    }
}

impl SuperAgentDynamicConfigStorer for SuperAgentConfigStore {
    fn store(&self, sub_agents: &SuperAgentDynamicConfig) -> Result<(), SuperAgentConfigError> {
        //TODO we should inject DirectoryManager and ensure the directory exists
        let _write_guard = self.rw_lock.write().unwrap();
        let Some(remote_path_file) = &self.remote_path else {
            unreachable!("we should not write into local paths");
        };
        Ok(serde_yaml::to_writer(
            std::fs::File::create(remote_path_file)?,
            sub_agents,
        )?)
    }
}

impl SuperAgentConfigStore {
    pub fn new(file_path: &Path) -> Self {
        let config_builder = Config::builder()
            // Pass default config file location and optionally, so we could pass all config through
            // env vars and no file!
            .add_source(File::new(&file_path.to_string_lossy(), FileFormat::Yaml).required(false))
            // Add in settings from the environment (with a prefix of `NR_` and separator double underscore, `__`)
            // Eg.. `NR_LOG__USE_DEBUG=1 ./target/app` would set the `log.use_debug` key
            // We use double underscore because we already use snake_case for the config keys. TODO to change?
            .add_source(
                Environment::with_prefix("NR_SA")
                    .prefix_separator("_")
                    .separator("__"),
            );

        Self {
            local_path: file_path.to_path_buf(),
            remote_path: None,
            config_builder,
            rw_lock: RwLock::new(()),
        }
    }

    // with_remote is supported for onhost implementation only and to make sure it is not used
    // we avoid to compile it for k8s
    #[cfg(feature = "onhost")]
    pub fn with_remote(self) -> Self {
        let remote_path = format!(
            "{}/{}",
            crate::super_agent::defaults::SUPER_AGENT_DATA_DIR(),
            "config.yaml"
        );

        Self {
            remote_path: Some(Path::new(&remote_path).to_path_buf()),
            ..self
        }
    }

    pub fn config_path(&self) -> &Path {
        self.remote_path.as_ref().unwrap_or(&self.local_path)
    }

    /// Load configs from local and remote sources.
    /// The local sources are the local file defined when this structure was created and the
    /// environment variables. It is loaded from the `config_builder` structure.
    /// The remote source is an optional file that is only concerned with the `dynamic` field of the
    /// `SuperAgentConfig`.
    fn _load_config(&self) -> Result<SuperAgentConfig, ConfigStoreError> {
        let _read_guard = self.rw_lock.read().unwrap();

        let mut local_config = self
            .config_builder
            // `build_cloned` performs all the I/O operations from a reference to the builder
            .build_cloned()?
            // From the retrieved config, attempt to generate the `SuperAgentConfig`
            .try_deserialize::<SuperAgentConfig>()?;

        if let Some(remote_config_file) = &self.remote_path {
            if remote_config_file.as_path().exists() {
                let remote_config_file = std::fs::File::open(remote_config_file)?;
                let remote_config = serde_yaml::from_reader(remote_config_file)
                    .map_err(|err| warn!("Unable to parse remote config: {}", err))
                    .ok();

                if let Some(remote_config) = remote_config {
                    // replace local agents with remote ones
                    local_config.dynamic = remote_config;
                }
            }
        }

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
