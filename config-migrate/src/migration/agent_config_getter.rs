use newrelic_super_agent::super_agent::config::{
    AgentTypeFQN, SuperAgentConfigError, SuperAgentDynamicConfig,
};
use newrelic_super_agent::super_agent::config_storer::storer::SuperAgentDynamicConfigLoader;
use newrelic_super_agent::super_agent::config_storer::SuperAgentConfigStoreFile;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConversionError {
    #[error("`{0}`")]
    SuperAgentConfigError(#[from] SuperAgentConfigError),
    #[error("No agents of type found on config")]
    NoAgentsFound,
}

pub struct AgentConfigGetter<SL = SuperAgentConfigStoreFile>
where
    SL: SuperAgentDynamicConfigLoader,
{
    pub(super) sub_agents_config_loader: SL,
}

#[cfg_attr(test, mockall::automock)]
impl<SL> AgentConfigGetter<SL>
where
    SL: SuperAgentDynamicConfigLoader + 'static,
{
    pub fn new(sub_agents_config_loader: SL) -> Self {
        Self {
            sub_agents_config_loader,
        }
    }
    pub fn get_agents_of_type(
        &self,
        agent_type: AgentTypeFQN,
    ) -> Result<SuperAgentDynamicConfig, ConversionError> {
        let mut super_agent_dynamic_config = self.sub_agents_config_loader.load()?;

        for agent in super_agent_dynamic_config.agents.clone() {
            if agent.1.agent_type != agent_type {
                super_agent_dynamic_config.agents.remove(&agent.0);
            }
        }
        if super_agent_dynamic_config.agents.is_empty() {
            return Err(ConversionError::NoAgentsFound);
        }

        Ok(super_agent_dynamic_config)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use mockall::mock;
    use newrelic_super_agent::super_agent::config::{
        AgentID, AgentTypeFQN, SubAgentConfig, SuperAgentDynamicConfig,
    };
    use std::collections::HashMap;

    mock! {
        pub SuperAgentDynamicConfigLoaderMock {}
        impl SuperAgentDynamicConfigLoader for SuperAgentDynamicConfigLoaderMock {
            fn load(&self) -> Result<SuperAgentDynamicConfig, SuperAgentConfigError>;
        }
    }

    #[test]
    fn load_agents_of_type() {
        let agent_type_fqn = AgentTypeFQN::from("com.newrelic.infrastructure_agent:0.0.2");
        let agents_cfg = r#"
agents:
  infra-agent-a:
    agent_type: "com.newrelic.infrastructure_agent:0.0.2"
  infra-agent-b:
    agent_type: "com.newrelic.infrastructure_agent:0.0.2"
  not-infra-agent:
    agent_type: "io.opentelemetry.collector:0.0.1"
"#;
        let mut config_loader = MockSuperAgentDynamicConfigLoaderMock::new();
        config_loader.expect_load().times(1).returning(move || {
            Ok(serde_yaml::from_str::<SuperAgentDynamicConfig>(agents_cfg).unwrap())
        });

        let config_getter = AgentConfigGetter::new(config_loader);
        let actual = config_getter.get_agents_of_type(agent_type_fqn);

        let expected = SuperAgentDynamicConfig {
            agents: HashMap::from([
                (
                    AgentID::new("infra-agent-a").unwrap(),
                    SubAgentConfig {
                        agent_type: AgentTypeFQN::from("com.newrelic.infrastructure_agent:0.0.2"),
                    },
                ),
                (
                    AgentID::new("infra-agent-b").unwrap(),
                    SubAgentConfig {
                        agent_type: AgentTypeFQN::from("com.newrelic.infrastructure_agent:0.0.2"),
                    },
                ),
            ]),
        };

        assert!(actual.is_ok());
        assert_eq!(actual.unwrap(), expected);
    }

    #[test]
    fn load_agents_of_type_error() {
        let agent_type_fqn = AgentTypeFQN::from("com.newrelic.infrastructure_agent:0.1.0");
        let agents_cfg = r#"
agents:
  infra-agent-a:
    agent_type: "com.newrelic.infrastructure_agent:0.0.2"
  infra-agent-b:
    agent_type: "com.newrelic.infrastructure_agent:0.0.2"
  not-infra-agent:
    agent_type: "io.opentelemetry.collector:0.0.1"
"#;
        let mut config_loader = MockSuperAgentDynamicConfigLoaderMock::new();
        config_loader.expect_load().times(1).returning(move || {
            Ok(serde_yaml::from_str::<SuperAgentDynamicConfig>(agents_cfg).unwrap())
        });

        let config_getter = AgentConfigGetter::new(config_loader);
        let result = config_getter.get_agents_of_type(agent_type_fqn);

        assert!(result.is_err())
    }
}
