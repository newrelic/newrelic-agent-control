use semver::Version;
use std::collections::HashMap;
use std::sync::Arc;

use crate::agent_control::config::AgentID;
use crate::agent_type::agent_metadata::AgentMetadata;
use crate::agent_type::definition::{AgentType, VariableTree};
use crate::agent_type::runtime_config::Runtime;
use crate::opamp::remote_config::status_manager::ConfigStatusManager;
use crate::opamp::remote_config::ConfigurationMap;

use super::error::LoaderError;
use super::loader::EffectiveConfigLoader;

/// Loader for effective configuration of a sub-agent.
#[derive(Debug)]
pub struct SubAgentEffectiveConfigLoader<M>
where
    M: ConfigStatusManager,
{
    agent_id: AgentID,
    config_manager: Arc<M>,
}

impl<M> SubAgentEffectiveConfigLoader<M>
where
    M: ConfigStatusManager,
{
    pub fn new(agent_id: AgentID, config_manager: Arc<M>) -> Self {
        Self {
            agent_id,
            config_manager,
        }
    }
}

impl<M> EffectiveConfigLoader for SubAgentEffectiveConfigLoader<M>
where
    M: ConfigStatusManager + Send + Sync + 'static,
{
    fn load(&self) -> Result<ConfigurationMap, LoaderError> {
        // TODO this gets removed after refactor PR. Is only used for capabilities has_remote.
        let fake_agent_type = AgentType::new(
            AgentMetadata {
                name: "".into(),
                namespace: "".into(),
                version: Version::new(0, 0, 0),
            },
            VariableTree::default(),
            Runtime::default(),
        );

        let values = self
            .config_manager
            .load_remote_fallback_local(&self.agent_id, &fake_agent_type.get_capabilities())
            .map_err(|err| {
                LoaderError::from(format!("loading {} config values: {}", &self.agent_id, err))
            })?;

        let values_string: String = values.try_into().map_err(|err| {
            LoaderError::from(format!(
                "converting {} config values to effective config: {}",
                &self.agent_id, err
            ))
        })?;

        // OpAMP effective config expects an empty key whenever there is only one config for an agent.
        let effective_config =
            ConfigurationMap::new(HashMap::from([(String::from(""), values_string)]));

        Ok(effective_config)
    }
}

#[cfg(test)]
mod tests {
    use mockall::predicate;

    use crate::agent_control::config::AgentID;
    use crate::agent_control::defaults::default_capabilities;
    use crate::opamp::effective_config::loader::EffectiveConfigLoader;
    use crate::opamp::effective_config::sub_agent::SubAgentEffectiveConfigLoader;
    use crate::opamp::remote_config::hash::Hash;
    use crate::opamp::remote_config::status::AgentRemoteConfigStatus;
    use crate::opamp::remote_config::status_manager::error::ConfigStatusManagerError;
    use crate::opamp::remote_config::status_manager::tests::MockConfigStatusManagerMock;
    use crate::opamp::remote_config::ConfigurationMap;
    use crate::values::yaml_config::YAMLConfig;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn test_agent() -> AgentID {
        AgentID::new("test-agent").unwrap()
    }
    #[test]
    fn test_effective_config_success_cases() {
        struct TestCase {
            name: &'static str,
            yaml_config: &'static str,
            expected_config: &'static str,
        }
        impl TestCase {
            fn run(self) {
                let agent_id = test_agent();
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
                    .with(predicate::eq(agent_id.clone()), predicate::eq(capabilities))
                    .return_once(|_, _| Ok(Some(remote_config)));

                self.assert("load_from_remote", config_manager);

                // Prepare the mock repository to load from fallback local
                let mut config_manager = MockConfigStatusManagerMock::new();
                config_manager
                    .expect_retrieve_remote_status()
                    .with(predicate::eq(agent_id.clone()), predicate::eq(capabilities))
                    .return_once(|_, _| Ok(None));
                config_manager
                    .expect_retrieve_local_config()
                    .with(predicate::eq(agent_id))
                    .return_once(|_| Ok(Some(yaml_config)));

                self.assert("load_fallback_local", config_manager);
            }

            fn assert(&self, scenario: &str, config_manager: MockConfigStatusManagerMock) {
                let loader =
                    SubAgentEffectiveConfigLoader::new(test_agent(), Arc::new(config_manager));

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
                name: "valid yaml",
                yaml_config: "fake-config: value",
                expected_config: "fake-config: value\n",
            },
            TestCase {
                name: "empty config",
                yaml_config: "",
                expected_config: "",
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    #[test]
    fn test_sa_effective_config_load_error() {
        let agent_id = test_agent();
        let capabilities = default_capabilities();

        // Prepare the mock repository to load from remote
        let mut config_manager = MockConfigStatusManagerMock::new();
        config_manager
            .expect_retrieve_remote_status()
            .with(predicate::eq(agent_id.clone()), predicate::eq(capabilities))
            .returning(|_, _| {
                Err(ConfigStatusManagerError::Retrieval(
                    "load error".to_string(),
                ))
            });

        let loader = SubAgentEffectiveConfigLoader::new(agent_id, Arc::new(config_manager));

        let load_error = loader.load().unwrap_err();

        assert!(load_error.to_string().contains("load error"))
    }
}
