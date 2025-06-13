use crate::agent_control::config::{AgentControlConfigError, AgentControlDynamicConfig};
use crate::agent_control::config_repository::repository::AgentControlDynamicConfigRepository;
use crate::agent_type::agent_type_id::AgentTypeID;
use semver::VersionReq;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConversionError {
    #[error("`{0}`")]
    AgentControlConfigError(#[from] AgentControlConfigError),
    #[error("Error comparing versions `{0}`")]
    SemverError(#[from] semver::Error),
    #[error("No agents of type found on config")]
    NoAgentsFound,
}

pub struct AgentConfigGetter<SL>
where
    SL: AgentControlDynamicConfigRepository,
{
    pub(super) sub_agents_config_loader: SL,
}

#[cfg_attr(test, mockall::automock)]
impl<SL> AgentConfigGetter<SL>
where
    SL: AgentControlDynamicConfigRepository + 'static,
{
    pub fn new(sub_agents_config_loader: SL) -> Self {
        Self {
            sub_agents_config_loader,
        }
    }
    pub fn get_agents_of_type_between_versions(
        &self,
        agent_type_min: AgentTypeID,
        agent_type_max: Option<AgentTypeID>,
    ) -> Result<AgentControlDynamicConfig, ConversionError> {
        let mut agent_control_dynamic_config = self.sub_agents_config_loader.load()?;
        let agent_type_namespace = agent_type_min.namespace();
        let agent_type_name = agent_type_min.name();

        // we calculate the versionReq pattern following a structure like: ">=1.2.3, <1.8.0"
        let version_req_min = format!(">={}", agent_type_min.version());
        let version_req_max = agent_type_max
            .map(|at| format!(", <{}", at.version()))
            .unwrap_or_default();
        let version_req =
            VersionReq::parse(format!("{}{}", version_req_min, version_req_max).as_str())?;

        for agent in agent_control_dynamic_config.agents.clone() {
            if agent.1.agent_type.namespace() != agent_type_namespace
                || agent.1.agent_type.name() != agent_type_name
                || !version_req.matches(agent.1.agent_type.version())
            {
                agent_control_dynamic_config.agents.remove(&agent.0);
            }
        }
        if agent_control_dynamic_config.agents.is_empty() {
            return Err(ConversionError::NoAgentsFound);
        }

        Ok(agent_control_dynamic_config)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::config::{AgentControlDynamicConfig, SubAgentConfig};
    use crate::opamp::remote_config::hash::ConfigState;
    use crate::values::config::RemoteConfig;
    use mockall::mock;
    use std::collections::HashMap;

    mock! {
        pub AgentControlDynamicConfigLoader {}
        impl AgentControlDynamicConfigRepository for AgentControlDynamicConfigLoader {
            fn load(&self) -> Result<AgentControlDynamicConfig, AgentControlConfigError>;

            fn store(&self, config: &RemoteConfig) -> Result<(), AgentControlConfigError>;

            fn update_state(&self, state: &ConfigState) -> Result<(), AgentControlConfigError>;

            fn get_remote_config(&self) -> Result<Option<RemoteConfig>, AgentControlConfigError>;

            fn delete(&self) -> Result<(), AgentControlConfigError>;
        }
    }

    #[test]
    fn load_agents_of_type_between_versions() {
        struct TestCase {
            name: &'static str,
            agent_type_id: AgentTypeID,
            next: Option<AgentTypeID>,
            agents_cfg: &'static str,
            expected: AgentControlDynamicConfig,
        }
        impl TestCase {
            fn run(self) {
                let mut config_loader = MockAgentControlDynamicConfigLoader::new();
                config_loader.expect_load().times(1).returning(move || {
                    Ok(serde_yaml::from_str::<AgentControlDynamicConfig>(self.agents_cfg).unwrap())
                });

                let config_getter = AgentConfigGetter::new(config_loader);
                let actual = config_getter
                    .get_agents_of_type_between_versions(self.agent_type_id, self.next);

                assert!(actual.is_ok());
                assert_eq!(actual.unwrap(), self.expected, "{}", self.name);
            }
        }
        let test_cases = vec![
            TestCase {
                name: "get only two matching between versions",
                agent_type_id: AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.0.1")
                    .unwrap(),
                next: Some(
                    AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:1.0.0").unwrap(),
                ),
                agents_cfg: r#"
agents:
  infra-agent-a:
    agent_type: "newrelic/com.newrelic.infrastructure:0.0.2"
  infra-agent-b:
    agent_type: "newrelic/com.newrelic.infrastructure:0.0.3"
  infra-agent-c:
    agent_type: "newrelic/com.newrelic.infrastructure:1.0.3"
  not-infra-agent:
    agent_type: "newrelic/io.opentelemetry.collector:0.0.1"
"#,
                expected: AgentControlDynamicConfig {
                    agents: HashMap::from([
                        (
                            AgentID::new("infra-agent-a").unwrap(),
                            SubAgentConfig {
                                agent_type: AgentTypeID::try_from(
                                    "newrelic/com.newrelic.infrastructure:0.0.2",
                                )
                                .unwrap(),
                            },
                        ),
                        (
                            AgentID::new("infra-agent-b").unwrap(),
                            SubAgentConfig {
                                agent_type: AgentTypeID::try_from(
                                    "newrelic/com.newrelic.infrastructure:0.0.3",
                                )
                                .unwrap(),
                            },
                        ),
                    ]),
                    chart_version: None,
                },
            },
            TestCase {
                name: "get all three matching since version",
                agent_type_id: AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.0.1")
                    .unwrap(),
                next: None,
                agents_cfg: r#"
agents:
  infra-agent-a:
    agent_type: "newrelic/com.newrelic.infrastructure:0.0.2"
  infra-agent-b:
    agent_type: "newrelic/com.newrelic.infrastructure:0.0.3"
  infra-agent-c:
    agent_type: "newrelic/com.newrelic.infrastructure:1.0.3"
  not-infra-agent:
    agent_type: "newrelic/io.opentelemetry.collector:0.0.1"
"#,
                expected: AgentControlDynamicConfig {
                    agents: HashMap::from([
                        (
                            AgentID::new("infra-agent-a").unwrap(),
                            SubAgentConfig {
                                agent_type: AgentTypeID::try_from(
                                    "newrelic/com.newrelic.infrastructure:0.0.2",
                                )
                                .unwrap(),
                            },
                        ),
                        (
                            AgentID::new("infra-agent-b").unwrap(),
                            SubAgentConfig {
                                agent_type: AgentTypeID::try_from(
                                    "newrelic/com.newrelic.infrastructure:0.0.3",
                                )
                                .unwrap(),
                            },
                        ),
                        (
                            AgentID::new("infra-agent-c").unwrap(),
                            SubAgentConfig {
                                agent_type: AgentTypeID::try_from(
                                    "newrelic/com.newrelic.infrastructure:1.0.3",
                                )
                                .unwrap(),
                            },
                        ),
                    ]),
                    chart_version: None,
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
            agent_type_id: AgentTypeID,
            next: Option<AgentTypeID>,
            agents_cfg: &'static str,
        }
        impl TestCase {
            fn run(self) {
                let mut config_loader = MockAgentControlDynamicConfigLoader::new();
                config_loader.expect_load().times(1).returning(move || {
                    Ok(serde_yaml::from_str::<AgentControlDynamicConfig>(self.agents_cfg).unwrap())
                });

                let config_getter = AgentConfigGetter::new(config_loader);
                let actual = config_getter
                    .get_agents_of_type_between_versions(self.agent_type_id, self.next);

                assert!(actual.is_err(), "{}", self.name)
            }
        }
        let test_cases = vec![
            TestCase {
                name: "error no agents higher or equal to version",
                agent_type_id: AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.1.0")
                    .unwrap(),
                next: None,
                agents_cfg: r#"
agents:
  infra-agent-a:
    agent_type: "newrelic/com.newrelic.infrastructure:0.0.2"
  infra-agent-b:
    agent_type: "newrelic/com.newrelic.infrastructure:0.0.2"
  not-infra-agent:
    agent_type: "newrelic/io.opentelemetry.collector:0.0.1"
"#,
            },
            TestCase {
                name: "error no agents of namespace",
                agent_type_id: AgentTypeID::try_from(
                    "francisco-partners/com.newrelic.infrastructure:0.0.1",
                )
                .unwrap(),
                next: None,
                agents_cfg: r#"
agents:
  infra-agent-a:
    agent_type: "newrelic/com.newrelic.infrastructure:0.0.2"
  infra-agent-b:
    agent_type: "newrelic/com.newrelic.infrastructure:0.0.3"
  infra-agent-c:
    agent_type: "newrelic/com.newrelic.infrastructure:1.0.3"
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
