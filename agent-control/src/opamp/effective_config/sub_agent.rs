use std::collections::HashMap;
use std::sync::Arc;

use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::default_capabilities;
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::agent_type::definition::{AgentType, VariableTree};
use crate::agent_type::runtime_config::{Deployment, Runtime};
use crate::opamp::remote_config::ConfigurationMap;
use crate::values::yaml_config_repository::{load_remote_fallback_local, YAMLConfigRepository};

use super::error::LoaderError;
use super::loader::EffectiveConfigLoader;

/// Loader for effective configuration of a sub-agent.
#[derive(Debug)]
pub struct SubAgentEffectiveConfigLoader<Y>
where
    Y: YAMLConfigRepository,
{
    agent_id: AgentID,
    yaml_config_repository: Arc<Y>,
}

impl<Y> SubAgentEffectiveConfigLoader<Y>
where
    Y: YAMLConfigRepository,
{
    pub fn new(agent_id: AgentID, yaml_config_repository: Arc<Y>) -> Self {
        Self {
            agent_id,
            yaml_config_repository,
        }
    }
}

impl<Y> EffectiveConfigLoader for SubAgentEffectiveConfigLoader<Y>
where
    Y: YAMLConfigRepository,
{
    fn load(&self) -> Result<ConfigurationMap, LoaderError> {
        // TODO this gets removed after refactor PR. Is only used for capabilities has_remote.
        let fake_agent_type = AgentType::new(
            AgentTypeID::try_from("namespace/name:0.0.1").unwrap(),
            VariableTree::default(),
            Runtime {
                deployment: Deployment::default(),
            },
        );

        let values = load_remote_fallback_local(
            self.yaml_config_repository.as_ref(),
            &self.agent_id,
            &default_capabilities(),
        )
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
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::defaults::default_capabilities;
    use crate::opamp::effective_config::loader::EffectiveConfigLoader;
    use crate::opamp::effective_config::sub_agent::SubAgentEffectiveConfigLoader;
    use crate::opamp::remote_config::ConfigurationMap;
    use crate::values::yaml_config::YAMLConfig;
    use crate::values::yaml_config_repository::tests::MockYAMLConfigRepositoryMock;
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

                // Prepare the mock repository to load from remote
                let mut yaml_config_repository = MockYAMLConfigRepositoryMock::default();
                yaml_config_repository.should_load_remote(
                    &agent_id,
                    capabilities,
                    &YAMLConfig::try_from(String::from(self.yaml_config)).unwrap(),
                );

                self.assert("load_from_remote", yaml_config_repository);

                // Prepare the mock repository to load from fallback local
                let mut yaml_config_repository = MockYAMLConfigRepositoryMock::default();
                yaml_config_repository
                    .expect_load_remote()
                    .once()
                    .returning(move |agent_id, c| {
                        assert_eq!(c, &capabilities);
                        assert_eq!(agent_id, &test_agent());
                        Ok(None)
                    });
                yaml_config_repository
                    .expect_load_local()
                    .once()
                    .returning(move |agent_id| {
                        assert_eq!(agent_id, &test_agent());
                        Ok(Some(
                            YAMLConfig::try_from(String::from(self.yaml_config)).unwrap(),
                        ))
                    });

                self.assert("load_fallback_local", yaml_config_repository);
            }

            fn assert(&self, scenario: &str, yaml_config_repository: MockYAMLConfigRepositoryMock) {
                let loader = SubAgentEffectiveConfigLoader::new(
                    test_agent(),
                    Arc::new(yaml_config_repository),
                );

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
        let mut yaml_config_repository = MockYAMLConfigRepositoryMock::default();
        yaml_config_repository.should_not_load_remote(&agent_id, capabilities);

        let loader = SubAgentEffectiveConfigLoader::new(agent_id, Arc::new(yaml_config_repository));

        let load_error = loader.load().unwrap_err();

        assert!(load_error.to_string().contains("load error"))
    }
}
