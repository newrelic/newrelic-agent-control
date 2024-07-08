use super::values::values_repository::ValuesRepository;
use crate::agent_type::agent_metadata::AgentMetadata;
use crate::agent_type::definition::{AgentType, VariableTree};
use crate::agent_type::runtime_config::Runtime;
use crate::opamp::effective_config::error::LoaderError;
use crate::opamp::effective_config::loader::EffectiveConfigLoader;
use crate::opamp::remote_config::ConfigurationMap;
use crate::super_agent::config::AgentID;
use semver::Version;
use std::collections::HashMap;
use std::sync::Arc;

/// Loader for effective configuration of a sub-agent.
#[derive(Debug)]
pub struct SubAgentEffectiveConfigLoader<VR>
where
    VR: ValuesRepository,
{
    agent_id: AgentID,
    values_repository: Arc<VR>,
}

impl<VR> SubAgentEffectiveConfigLoader<VR>
where
    VR: ValuesRepository,
{
    pub fn new(agent_id: AgentID, values_repository: Arc<VR>) -> Self {
        Self {
            agent_id,
            values_repository,
        }
    }
}

impl<VR> EffectiveConfigLoader for SubAgentEffectiveConfigLoader<VR>
where
    VR: ValuesRepository,
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
            .values_repository
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
    use crate::agent_type::agent_values::AgentValues;
    use crate::agent_type::definition::{AgentType, VariableTree};
    use crate::agent_type::runtime_config::Runtime;
    use crate::opamp::effective_config::loader::EffectiveConfigLoader;
    use crate::opamp::remote_config::ConfigurationMap;
    use crate::sub_agent::effective_config::SubAgentEffectiveConfigLoader;
    use crate::sub_agent::values::values_repository::test::MockRemoteValuesRepositoryMock;
    use crate::super_agent::config::AgentID;
    use semver::Version;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn test_load() {
        let agent_id = AgentID::new("test-agent").unwrap();
        let mut values_repository = MockRemoteValuesRepositoryMock::default();

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
        values_repository.should_load(
            &agent_id,
            &agent_type,
            &AgentValues::try_from(String::from("fake_config: value")).unwrap(),
        );

        let loader = SubAgentEffectiveConfigLoader {
            agent_id: agent_id.clone(),
            values_repository: Arc::new(values_repository),
        };

        let effective_config = loader.load().unwrap();

        let expected_config =
            ConfigurationMap::new(HashMap::from([("".into(), "fake_config: value\n".into())]));

        assert_eq!(effective_config, expected_config);
    }
}
