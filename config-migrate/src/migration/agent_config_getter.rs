use newrelic_super_agent::config::error::SuperAgentConfigError;
use newrelic_super_agent::config::store::{SubAgentsConfigLoader, SuperAgentConfigStoreFile};
use newrelic_super_agent::config::super_agent_configs::{AgentTypeFQN, SubAgentsConfig};
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
    SL: SubAgentsConfigLoader,
{
    pub(super) sub_agents_config_loader: SL,
}

#[cfg_attr(test, mockall::automock)]
impl<SL> AgentConfigGetter<SL>
where
    SL: SubAgentsConfigLoader + 'static,
{
    pub fn new(sub_agents_config_loader: SL) -> Self {
        Self {
            sub_agents_config_loader,
        }
    }
    pub fn get_agents_of_type(
        &self,
        agent_type: AgentTypeFQN,
    ) -> Result<SubAgentsConfig, ConversionError> {
        let mut agents_config = self.sub_agents_config_loader.load()?;

        for agent in agents_config.agents.clone() {
            if agent.1.agent_type != agent_type {
                agents_config.remove(&agent.0);
            }
        }
        if agents_config.agents.is_empty() {
            return Err(ConversionError::NoAgentsFound);
        }

        Ok(agents_config)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use mockall::mock;
    use newrelic_super_agent::config::super_agent_configs::{AgentID, SubAgentConfig};
    use std::collections::HashMap;

    mock! {
        pub SubAgentsConfigLoaderMock {}
        impl SubAgentsConfigLoader for SubAgentsConfigLoaderMock {
            fn load(&self) -> Result<SubAgentsConfig, SuperAgentConfigError>;
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
        let mut config_loader = MockSubAgentsConfigLoaderMock::new();
        config_loader
            .expect_load()
            .times(1)
            .returning(move || Ok(serde_yaml::from_str::<SubAgentsConfig>(agents_cfg).unwrap()));

        let config_getter = AgentConfigGetter::new(config_loader);
        let actual = config_getter.get_agents_of_type(agent_type_fqn);

        let expected = SubAgentsConfig {
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
            ])
            .into(),
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
        let mut config_loader = MockSubAgentsConfigLoaderMock::new();
        config_loader
            .expect_load()
            .times(1)
            .returning(move || Ok(serde_yaml::from_str::<SubAgentsConfig>(agents_cfg).unwrap()));

        let config_getter = AgentConfigGetter::new(config_loader);
        let result = config_getter.get_agents_of_type(agent_type_fqn);

        assert!(result.is_err())
    }
}
