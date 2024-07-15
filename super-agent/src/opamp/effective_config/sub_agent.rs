use semver::Version;
use std::collections::HashMap;
use std::sync::Arc;

use crate::agent_type::agent_metadata::AgentMetadata;
use crate::agent_type::definition::{AgentType, VariableTree};
use crate::agent_type::runtime_config::Runtime;
use crate::opamp::remote_config::ConfigurationMap;
use crate::super_agent::config::AgentID;
use crate::values::yaml_config_repository::YAMLConfigRepository;

use super::error::LoaderError;
use super::loader::EffectiveConfigLoader;

/// Loader for effective configuration of a sub-agent.
#[derive(Debug)]
pub struct SubAgentEffectiveConfigLoader<VR>
where
    VR: YAMLConfigRepository,
{
    agent_id: AgentID,
    yaml_config_repository: Arc<VR>,
}

impl<VR> SubAgentEffectiveConfigLoader<VR>
where
    VR: YAMLConfigRepository,
{
    pub fn new(agent_id: AgentID, yaml_config_repository: Arc<VR>) -> Self {
        Self {
            agent_id,
            yaml_config_repository,
        }
    }
}

impl<VR> EffectiveConfigLoader for SubAgentEffectiveConfigLoader<VR>
where
    VR: YAMLConfigRepository,
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
            .yaml_config_repository
            .load(&self.agent_id, &fake_agent_type)
            .map_err(|err| {
                LoaderError::from(format!("loading {} config values: {}", &self.agent_id, err))
            })?;

        let values_string: String = values.try_into().map_err(|err| {
            LoaderError::from(format!(
                "converting {} config values to effective config: {}",
                &self.agent_id, err
            ))
        })?;

        let effective_config =
            ConfigurationMap::new(HashMap::from([(String::from(""), values_string)]));

        Ok(effective_config)
    }
}

#[cfg(test)]
mod test {
    use crate::agent_type::agent_metadata::AgentMetadata;
    use crate::agent_type::definition::{AgentType, VariableTree};
    use crate::agent_type::runtime_config::Runtime;
    use crate::opamp::effective_config::loader::EffectiveConfigLoader;
    use crate::opamp::effective_config::sub_agent::SubAgentEffectiveConfigLoader;
    use crate::opamp::remote_config::ConfigurationMap;
    use crate::super_agent::config::AgentID;
    use crate::values::yaml_config::YAMLConfig;
    use crate::values::yaml_config_repository::test::MockYAMLConfigRepositoryMock;
    use semver::Version;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn test_load() {
        let agent_id = AgentID::new("test-agent").unwrap();
        let mut yaml_config_repository = MockYAMLConfigRepositoryMock::default();

        // TODO remove after refactor of values repository
        let agent_type = AgentType::new(
            AgentMetadata {
                name: "".into(),
                namespace: "".into(),
                version: Version::new(0, 0, 0),
            },
            VariableTree::default(),
            Runtime::default(),
        );
        yaml_config_repository.should_load(
            &agent_id,
            &agent_type,
            &YAMLConfig::try_from(String::from("fake_config: value")).unwrap(),
        );

        let loader = SubAgentEffectiveConfigLoader {
            agent_id: agent_id.clone(),
            yaml_config_repository: Arc::new(yaml_config_repository),
        };

        let effective_config = loader.load().unwrap();

        let expected_config =
            ConfigurationMap::new(HashMap::from([("".into(), "fake_config: value\n".into())]));

        assert_eq!(effective_config, expected_config);
    }
}
