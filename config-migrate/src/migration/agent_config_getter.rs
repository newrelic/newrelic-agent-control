use newrelic_super_agent::super_agent::config::{
    AgentTypeFQN, SuperAgentConfigError, SuperAgentDynamicConfig,
};
use newrelic_super_agent::super_agent::config_storer::storer::SuperAgentDynamicConfigLoader;
use newrelic_super_agent::super_agent::config_storer::SuperAgentConfigStoreFile;
use semver::{Version, VersionReq};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConversionError {
    #[error("`{0}`")]
    SuperAgentConfigError(#[from] SuperAgentConfigError),
    #[error("Error comparing versions `{0}`")]
    SemverError(#[from] semver::Error),
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
    pub fn get_agents_of_type_between_versions(
        &self,
        agent_type_min: AgentTypeFQN,
        agent_type_max: Option<AgentTypeFQN>,
    ) -> Result<SuperAgentDynamicConfig, ConversionError> {
        let mut super_agent_dynamic_config = self.sub_agents_config_loader.load()?;
        let agent_type_namespace = agent_type_min.namespace();
        let agent_type_name = agent_type_min.name();

        // we calculate the versionReq pattern following a structure like: ">=1.2.3, <1.8.0"
        let version_req_min = format!(">={}", agent_type_min.version());
        let version_req_max = agent_type_max
            .map(|at| format!(", <{}", at.version()))
            .unwrap_or_default();
        let version_req =
            VersionReq::parse(format!("{}{}", version_req_min, version_req_max).as_str())?;

        for agent in super_agent_dynamic_config.agents.clone() {
            let agent_version = Version::parse(agent.1.agent_type.version().as_str()).unwrap();

            if agent.1.agent_type.namespace() != agent_type_namespace
                || agent.1.agent_type.name() != agent_type_name
                || !version_req.matches(&agent_version)
            {
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
    fn load_agents_of_type_between_versions() {
        struct TestCase {
            name: &'static str,
            agent_type_fqn: AgentTypeFQN,
            next: Option<AgentTypeFQN>,
            agents_cfg: &'static str,
            expected: SuperAgentDynamicConfig,
        }
        impl TestCase {
            fn run(self) {
                let mut config_loader = MockSuperAgentDynamicConfigLoaderMock::new();
                config_loader.expect_load().times(1).returning(move || {
                    Ok(serde_yaml::from_str::<SuperAgentDynamicConfig>(self.agents_cfg).unwrap())
                });

                let config_getter = AgentConfigGetter::new(config_loader);
                let actual = config_getter
                    .get_agents_of_type_between_versions(self.agent_type_fqn, self.next);

                assert!(actual.is_ok());
                assert_eq!(actual.unwrap(), self.expected, "{}", self.name);
            }
        }
        let test_cases = vec![
            TestCase {
                name: "get only two matching between versions",
                agent_type_fqn: AgentTypeFQN::from(
                    "newrelic/com.newrelic.infrastructure_agent:0.0.1",
                ),
                next: Some(AgentTypeFQN::from(
                    "newrelic/com.newrelic.infrastructure_agent:1.0.0",
                )),
                agents_cfg: r#"
agents:
  infra-agent-a:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:0.0.2"
  infra-agent-b:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:0.0.3"
  infra-agent-c:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:1.0.3"
  not-infra-agent:
    agent_type: "newrelic/io.opentelemetry.collector:0.0.1"
"#,
                expected: SuperAgentDynamicConfig {
                    agents: HashMap::from([
                        (
                            AgentID::new("infra-agent-a").unwrap(),
                            SubAgentConfig {
                                agent_type: AgentTypeFQN::from(
                                    "newrelic/com.newrelic.infrastructure_agent:0.0.2",
                                ),
                            },
                        ),
                        (
                            AgentID::new("infra-agent-b").unwrap(),
                            SubAgentConfig {
                                agent_type: AgentTypeFQN::from(
                                    "newrelic/com.newrelic.infrastructure_agent:0.0.3",
                                ),
                            },
                        ),
                    ]),
                },
            },
            TestCase {
                name: "get all three matching since version",
                agent_type_fqn: AgentTypeFQN::from(
                    "newrelic/com.newrelic.infrastructure_agent:0.0.1",
                ),
                next: None,
                agents_cfg: r#"
agents:
  infra-agent-a:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:0.0.2"
  infra-agent-b:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:0.0.3"
  infra-agent-c:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:1.0.3"
  not-infra-agent:
    agent_type: "newrelic/io.opentelemetry.collector:0.0.1"
"#,
                expected: SuperAgentDynamicConfig {
                    agents: HashMap::from([
                        (
                            AgentID::new("infra-agent-a").unwrap(),
                            SubAgentConfig {
                                agent_type: AgentTypeFQN::from(
                                    "newrelic/com.newrelic.infrastructure_agent:0.0.2",
                                ),
                            },
                        ),
                        (
                            AgentID::new("infra-agent-b").unwrap(),
                            SubAgentConfig {
                                agent_type: AgentTypeFQN::from(
                                    "newrelic/com.newrelic.infrastructure_agent:0.0.3",
                                ),
                            },
                        ),
                        (
                            AgentID::new("infra-agent-c").unwrap(),
                            SubAgentConfig {
                                agent_type: AgentTypeFQN::from(
                                    "newrelic/com.newrelic.infrastructure_agent:1.0.3",
                                ),
                            },
                        ),
                    ]),
                },
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    #[test]
    fn load_agents_of_type_error() {
        struct TestCase {
            name: &'static str,
            agent_type_fqn: AgentTypeFQN,
            next: Option<AgentTypeFQN>,
            agents_cfg: &'static str,
        }
        impl TestCase {
            fn run(self) {
                let mut config_loader = MockSuperAgentDynamicConfigLoaderMock::new();
                config_loader.expect_load().times(1).returning(move || {
                    Ok(serde_yaml::from_str::<SuperAgentDynamicConfig>(self.agents_cfg).unwrap())
                });

                let config_getter = AgentConfigGetter::new(config_loader);
                let actual = config_getter
                    .get_agents_of_type_between_versions(self.agent_type_fqn, self.next);

                assert!(actual.is_err(), "{}", self.name)
            }
        }
        let test_cases = vec![
            TestCase {
                name: "error no agents higher or equal to version",
                agent_type_fqn: AgentTypeFQN::from(
                    "newrelic/com.newrelic.infrastructure_agent:0.1.0",
                ),
                next: None,
                agents_cfg: r#"
agents:
  infra-agent-a:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:0.0.2"
  infra-agent-b:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:0.0.2"
  not-infra-agent:
    agent_type: "newrelic/io.opentelemetry.collector:0.0.1"
"#,
            },
            TestCase {
                name: "error no agents of namespace",
                agent_type_fqn: AgentTypeFQN::from(
                    "francisco-partners/com.newrelic.infrastructure_agent:0.0.1",
                ),
                next: None,
                agents_cfg: r#"
agents:
  infra-agent-a:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:0.0.2"
  infra-agent-b:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:0.0.3"
  infra-agent-c:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:1.0.3"
  not-infra-agent:
    agent_type: "newrelic/io.opentelemetry.collector:0.0.1"
"#,
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }
}
