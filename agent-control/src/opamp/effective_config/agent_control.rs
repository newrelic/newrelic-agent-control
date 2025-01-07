use opamp_client::operation::capabilities::Capabilities;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

use crate::agent_control::config::AgentID;
use crate::agent_control::defaults::default_capabilities;
use crate::opamp::remote_config::status_manager::ConfigStatusManager;
use crate::opamp::remote_config::ConfigurationMap;
use crate::values::yaml_config::YAMLConfig;

use super::error::LoaderError;
use super::loader::EffectiveConfigLoader;

/// Loader for effective configuration of a agent-control.
#[derive(Debug)]
pub struct AgentControlEffectiveConfigLoader<M>
where
    M: ConfigStatusManager,
{
    config_manager: Arc<M>,
    agent_id: AgentID,
    agent_control_capabilities: Capabilities,
}

impl<M> AgentControlEffectiveConfigLoader<M>
where
    M: ConfigStatusManager,
{
    pub fn new(config_manager: Arc<M>) -> Self {
        Self {
            config_manager,
            agent_id: AgentID::new_agent_control_id(),
            agent_control_capabilities: default_capabilities(),
        }
    }
}

/// The AgentControlEffectiveConfig represents the effective configuration of the agent control.
/// It is a subset of the agent control configuration that can be modified through opamp remote configs.
/// It doesn't contain any default values.
#[derive(Debug, Deserialize, Serialize)]
struct AgentControlEffectiveConfig {
    // Using Option since local 'agents' config could be set from env vars,
    // and this should not be a failure scenario.
    #[serde(skip_serializing_if = "Option::is_none")]
    agents: Option<serde_yaml::Value>,
}

#[derive(Debug, Error)]
pub enum AgentControlEffectiveConfigError {
    #[error("processing agent-control effective config: `{0}`")]
    Conversion(String),
}

impl TryFrom<YAMLConfig> for AgentControlEffectiveConfig {
    type Error = AgentControlEffectiveConfigError;

    fn try_from(value: YAMLConfig) -> Result<Self, Self::Error> {
        let config_string: String = value.try_into().map_err(|err| {
            AgentControlEffectiveConfigError::Conversion(format!(
                "converting effective config from stored values: {}",
                err
            ))
        })?;

        let effective_config = serde_yaml::from_str(&config_string).map_err(|err| {
            AgentControlEffectiveConfigError::Conversion(format!(
                "converting effective config: {}",
                err
            ))
        })?;

        Ok(effective_config)
    }
}

impl<M> EffectiveConfigLoader for AgentControlEffectiveConfigLoader<M>
where
    M: ConfigStatusManager + Send + Sync + 'static,
{
    fn load(&self) -> Result<ConfigurationMap, LoaderError> {
        // Given the effective config constraints mentionend in the `EffectiveConfigLoader` trait,
        // the agent control effective config will be composed of:
        // - The dynamic part of the agent control config
        // - Config set from environment variables will not be included in the effective config

        // For the effective config load, we can follow the load remote or fallback to local, since only the dynamic part is needed.

        let config = self
            .config_manager
            .load_remote_fallback_local(&self.agent_id, &self.agent_control_capabilities)
            .map_err(|err| {
                LoaderError::from(format!("loading {} config values: {}", &self.agent_id, err))
            })?;

        // Deserialize only effective config making sure that not default values are reported.
        let dynamic_config: AgentControlEffectiveConfig = config.try_into().map_err(|err| {
            LoaderError::from(format!(
                "building {} effective config: {}",
                &self.agent_id, err
            ))
        })?;

        let effective_config = ConfigurationMap::new(HashMap::from([(
            // OpAMP effective config expects an empty key whenever there is only one config for an agent.
            String::from(""),
            serde_yaml::to_string(&dynamic_config).map_err(|err| {
                LoaderError::from(format!(
                    "serializing {} effective config: {}",
                    &self.agent_id, err
                ))
            })?,
        )]));

        Ok(effective_config)
    }
}

#[cfg(test)]
mod tests {
    use mockall::predicate;

    use crate::agent_control::config::AgentID;
    use crate::agent_control::defaults::default_capabilities;
    use crate::opamp::effective_config::agent_control::AgentControlEffectiveConfigLoader;
    use crate::opamp::effective_config::loader::EffectiveConfigLoader;
    use crate::opamp::remote_config::hash::Hash;
    use crate::opamp::remote_config::status::AgentRemoteConfigStatus;
    use crate::opamp::remote_config::status_manager::error::ConfigStatusManagerError;
    use crate::opamp::remote_config::status_manager::tests::MockConfigStatusManagerMock;
    use crate::opamp::remote_config::ConfigurationMap;
    use crate::values::yaml_config::YAMLConfig;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn test_sa_effective_config_success_cases() {
        struct TestCase {
            name: &'static str,
            yaml_config: &'static str,
            expected_config: &'static str,
        }
        impl TestCase {
            fn run(self) {
                let agent_id = AgentID::new_agent_control_id();
                let capabilities = default_capabilities();

                let mut hash = Hash::new("some_hash".to_string());
                hash.apply();
                let yaml_config = YAMLConfig::try_from(String::from(self.yaml_config)).unwrap();
                let remote_config = AgentRemoteConfigStatus {
                    status_hash: hash,
                    remote_config: Some(yaml_config.clone()),
                };

                // Prepare the mock repository to load from remote
                let mut config_manager = MockConfigStatusManagerMock::new();
                config_manager
                    .expect_retrieve_remote_status()
                    .once()
                    .with(predicate::eq(agent_id.clone()), predicate::eq(capabilities))
                    .return_once(|_, _| Ok(Some(remote_config)));

                self.assert("load_from_remote", config_manager);

                // Prepare the mock repository to load from fallback local
                let mut config_manager = MockConfigStatusManagerMock::new();
                config_manager
                    .expect_retrieve_remote_status()
                    .once()
                    .with(predicate::eq(agent_id.clone()), predicate::eq(capabilities))
                    .return_once(|_, _| Ok(None));
                config_manager
                    .expect_retrieve_local_config()
                    .once()
                    .with(predicate::eq(agent_id))
                    .return_once(|_| Ok(Some(yaml_config)));

                self.assert("load_fallback_local", config_manager);
            }

            fn assert(&self, scenario: &str, config_manager: MockConfigStatusManagerMock) {
                let loader = AgentControlEffectiveConfigLoader::new(Arc::new(config_manager));

                let effective_config = loader.load().unwrap();

                let opamp_config = HashMap::from([("".into(), self.expected_config.into())]);
                let expected_config = ConfigurationMap::new(opamp_config);

                assert_eq!(
                    effective_config, expected_config,
                    "test case: {}-{}",
                    self.name, scenario
                );
            }
        }
        let test_cases = vec![
            TestCase {
                name: "only effective config is present",
                yaml_config: r#"
opamp:
  endpoint: https://fake.com/v1/opamp
  headers:
    api-key: fake-key
agents:
  fake-agent:
    agent_type: agent/type:0.0.1
"#,
                expected_config: r#"agents:
  fake-agent:
    agent_type: agent/type:0.0.1
"#,
            },
            TestCase {
                name: "effective config uses raw serealization",
                yaml_config: "agents: any serde_yaml value could be here",
                expected_config: "agents: any serde_yaml value could be here\n",
            },
            TestCase {
                name: "empty agets",
                yaml_config: "agents: {}",
                expected_config: "agents: {}\n",
            },
            TestCase {
                name: "missing agents do not fail",
                yaml_config: "fake-sa-config: value",
                expected_config: "{}\n",
            },
            TestCase {
                name: "empty config",
                yaml_config: "",
                expected_config: "{}\n",
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    #[test]
    fn test_sa_effective_config_load_error() {
        let agent_id = AgentID::new_agent_control_id();
        let capabilities = default_capabilities();

        // Prepare the mock repository to load from remote
        let mut config_manager = MockConfigStatusManagerMock::new();
        config_manager
            .expect_retrieve_remote_status()
            .with(predicate::eq(agent_id), predicate::eq(capabilities))
            .returning(|_, _| {
                Err(ConfigStatusManagerError::Retrieval(
                    "load error".to_string(),
                ))
            });

        let loader = AgentControlEffectiveConfigLoader::new(Arc::new(config_manager));

        let load_error = loader.load().unwrap_err();

        assert!(load_error.to_string().contains("load error"))
    }
}
