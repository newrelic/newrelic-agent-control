use std::path::Path;

use config::{builder::DefaultState, Config, ConfigBuilder, File, FileFormat};

use super::{agent_configs::SuperAgentConfig, error::SuperAgentConfigError};

/// Builder for the configuration, managing if it is loaded from a default expected file or from
/// a custom one provided by the command line arguments.
pub struct Resolver(ConfigBuilder<DefaultState>);

impl Resolver {
    // build from an arbitrary source only for tests
    #[cfg(test)]
    fn new<T>(source: T) -> Self
    where
        T: config::Source + Send + Sync + 'static,
    {
        let builder = Config::builder().add_source(source);
        Self(builder)
    }

    fn with_file_source(self, file: &Path) -> Self {
        Self(
            self.0
                .add_source(File::new(file.to_string_lossy().as_ref(), FileFormat::Yaml)),
        )
    }

    fn build_config(self) -> Result<SuperAgentConfig, SuperAgentConfigError> {
        Ok(self.0.build()?.try_deserialize::<SuperAgentConfig>()?)
    }

    /// Attempts to build the configuration
    pub fn retrieve_config(file: &Path) -> Result<SuperAgentConfig, SuperAgentConfigError> {
        Self::default().with_file_source(file).build_config()
    }
}

impl Default for Resolver {
    fn default() -> Self {
        let builder = Config::builder();
        Self(builder)
    }
}

#[cfg(test)]
mod tests {

    use std::{collections::HashMap, time::Duration};

    use super::*;
    use crate::config::{
        agent_configs::{
            AgentConfig, BackoffStrategyConfig, BackoffStrategyInner, RestartPolicyConfig,
            SuperAgentConfig,
        },
        resolver::Resolver,
    };
    use config::Value;

    #[test]
    fn resolve_one_agent() {
        // Build the config

        let actual = Resolver::new(File::from_str(
            "
# just Infra Agent enabled
agents:
  nr_infra_agent:
",
            FileFormat::Yaml,
        ))
        .build_config()
        .unwrap();

        let expected = SuperAgentConfig {
            agents: [("nr_infra_agent".to_string(), None)]
                .iter()
                .cloned()
                .collect(),
        };

        assert_eq!(actual.agents.len(), 1);
        assert_eq!(actual, expected);
    }

    #[test]
    fn resolve_two_different_agents() {
        // Build the config
        let actual = Resolver::new(File::from_str(
            "
# both enabled
agents:
  nr_infra_agent:
  nr_otel_collector:
",
            FileFormat::Yaml,
        ))
        .build_config()
        .unwrap();

        let expected = SuperAgentConfig {
            agents: [
                ("nr_infra_agent".to_string(), None),
                ("nr_otel_collector".to_string(), None),
            ]
            .iter()
            .cloned()
            .collect(),
        };

        assert_eq!(actual.agents.len(), 2);
        assert_eq!(actual, expected);
    }

    #[test]
    fn resolve_same_type_agents() {
        // Build the config
        let actual = Resolver::new(File::from_str(
            "
# both enabled
agents:
  nr_infra_agent:
  nr_otel_collector:
  nr_infra_agent/otherinstance:
",
            FileFormat::Yaml,
        ))
        .build_config()
        .unwrap();

        let expected = SuperAgentConfig {
            agents: [
                ("nr_infra_agent".to_string(), None),
                ("nr_otel_collector".to_string(), None),
                ("nr_infra_agent/otherinstance".to_string(), None),
            ]
            .iter()
            .cloned()
            .collect(),
        };

        assert_eq!(actual.agents.len(), 3);
        assert_eq!(actual, expected);
    }

    #[test]
    fn resolve_config_with_unexpected_fields() {
        let actual = Resolver::new(File::from_str(
            "
# just Infra Agent enabled
agents:
  nr_infra_agent:
this_is_another_random_config: value
",
            FileFormat::Yaml,
        ))
        .build_config();
        assert!(actual.is_err());
        assert!(actual
            .unwrap_err()
            .to_string()
            .contains("unknown field `this_is_another_random_config`"));
    }

    #[test]
    fn resolve_empty_agents_field_should_fail_without_empty_map() {
        assert!(Resolver::new(File::from_str(
            "
agents:
",
            FileFormat::Yaml,
        ))
        .build_config()
        .is_err());
    }

    #[test]
    fn resolve_empty_agents_field_good() {
        let actual = Resolver::new(File::from_str(
            r"
agents: {}
",
            FileFormat::Yaml,
        ))
        .build_config();

        let expected = SuperAgentConfig {
            agents: HashMap::new(),
        };

        assert!(actual.is_ok());
        assert_eq!(actual.unwrap(), expected);
    }

    #[test]
    fn resolve_agents_with_custom_configs() {
        // Build the config
        let actual = Resolver::new(File::from_str(
            "
agents:
  nr_infra_agent:
    config:
      configValue: value
      configList: [value1, value2]
      configMap:
        key1: value1
        key2: value2
  nr_otel_collector:
  nr_infra_agent/otherinstance:
    config:
      otherConfigValue: value
      otherConfigList: [value1, value2]
      otherConfigMap:
        key1: value1
        key2: value2
",
            FileFormat::Yaml,
        ))
        .build_config()
        .unwrap();

        // Deserializing with the serde_yaml crate because putting
        // the literal Value representations here is too verbose!
        let expected_nria_conf = serde_yaml::from_str::<HashMap<String, Value>>(
            r#"
            configValue: value
            configList: [value1, value2]
            configMap:
                key1: value1
                key2: value2
            "#,
        )
        .unwrap();
        let expected_otherinstance_nria_conf = serde_yaml::from_str::<HashMap<String, Value>>(
            r#"
            otherConfigValue: value
            otherConfigList: [value1, value2]
            otherConfigMap:
                key1: value1
                key2: value2
            "#,
        )
        .unwrap();

        let expected = SuperAgentConfig {
            agents: [
                (
                    "nr_infra_agent".to_string(),
                    Some(AgentConfig {
                        restart_policy: RestartPolicyConfig::default(),
                        config: Some(expected_nria_conf),
                    }),
                ),
                (
                    "nr_infra_agent/otherinstance".to_string(),
                    Some(AgentConfig {
                        restart_policy: RestartPolicyConfig::default(),
                        config: Some(expected_otherinstance_nria_conf),
                    }),
                ),
                ("nr_otel_collector".to_string(), None),
            ]
            .iter()
            .cloned()
            .collect(),
        };

        assert_eq!(actual.agents.len(), 3);
        assert_eq!(actual, expected);
    }

    #[test]
    fn resolve_default_backoff_strategy() {
        // Build the config

        let actual = Resolver::new(File::from_str(
            "
# just Infra Agent enabled
agents:
  nr_infra_agent:
    restart_policy: {{}}
",
            FileFormat::Yaml,
        ))
        .build_config()
        .unwrap();

        let expected = SuperAgentConfig {
            agents: [(
                "nr_infra_agent".to_string(),
                Some(AgentConfig {
                    restart_policy: RestartPolicyConfig {
                        backoff_strategy: BackoffStrategyConfig::Linear(
                            BackoffStrategyInner::default(),
                        ),
                        restart_exit_codes: vec![],
                    },
                    config: None,
                }),
            )]
            .iter()
            .cloned()
            .collect(),
        };

        assert_eq!(actual.agents.len(), 1);
        assert_eq!(actual, expected);
    }

    #[test]
    fn resolve_duration_seconds() {
        // Build the config

        let actual = Resolver::new(File::from_str(
            "
# just Infra Agent enabled
agents:
  nr_infra_agent:
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay_seconds: 1
        max_retries: 3
        last_retry_interval_seconds: 30
",
            FileFormat::Yaml,
        ))
        .build_config()
        .unwrap();

        let expected = SuperAgentConfig {
            agents: [(
                "nr_infra_agent".to_string(),
                Some(AgentConfig {
                    restart_policy: RestartPolicyConfig {
                        backoff_strategy: BackoffStrategyConfig::Fixed(BackoffStrategyInner {
                            backoff_delay_seconds: Duration::from_secs(1),
                            max_retries: 3,
                            last_retry_interval_seconds: Duration::from_secs(30),
                        }),
                        restart_exit_codes: vec![],
                    },
                    config: None,
                }),
            )]
            .iter()
            .cloned()
            .collect(),
        };

        assert_eq!(actual.agents.len(), 1);
        assert_eq!(actual, expected);
    }

    #[test]
    fn resolve_only_backoff_strategy_type() {
        // Build the config

        let actual = Resolver::new(File::from_str(
            "
# just Infra Agent enabled
agents:
  nr_infra_agent:
    restart_policy:
      backoff_strategy:
        type: fixed
",
            FileFormat::Yaml,
        ))
        .build_config()
        .unwrap();

        let expected = SuperAgentConfig {
            agents: [(
                "nr_infra_agent".to_string(),
                Some(AgentConfig {
                    restart_policy: RestartPolicyConfig {
                        backoff_strategy: BackoffStrategyConfig::Fixed(
                            BackoffStrategyInner::default(),
                        ),
                        restart_exit_codes: vec![],
                    },
                    config: None,
                }),
            )]
            .iter()
            .cloned()
            .collect(),
        };

        assert_eq!(actual.agents.len(), 1);
        assert_eq!(actual, expected);
    }

    #[test]
    fn resolve_backoff_strategy_none() {
        // Build the config

        let actual = Resolver::new(File::from_str(
            "
# just Infra Agent enabled
agents:
  nr_infra_agent:
    restart_policy:
      backoff_strategy:
        type: none
",
            FileFormat::Yaml,
        ))
        .build_config()
        .unwrap();

        let expected = SuperAgentConfig {
            agents: [(
                "nr_infra_agent".to_string(),
                Some(AgentConfig {
                    restart_policy: RestartPolicyConfig {
                        backoff_strategy: BackoffStrategyConfig::None,
                        restart_exit_codes: vec![],
                    },
                    config: None,
                }),
            )]
            .iter()
            .cloned()
            .collect(),
        };

        assert_eq!(actual.agents.len(), 1);
        assert_eq!(actual, expected);
    }

    #[test]
    fn backoff_strategy_none_ignores_additional_configs() {
        // Build the config

        let actual = Resolver::new(File::from_str(
            "
# just Infra Agent enabled
agents:
  nr_infra_agent:
    restart_policy:
      backoff_strategy:
        type: none
        backoff_delay_seconds: 1
        max_retries: 3
        last_retry_interval_seconds: 30
",
            FileFormat::Yaml,
        ))
        .build_config()
        .unwrap();

        let expected = SuperAgentConfig {
            agents: [(
                "nr_infra_agent".to_string(),
                Some(AgentConfig {
                    restart_policy: RestartPolicyConfig {
                        backoff_strategy: BackoffStrategyConfig::None,
                        restart_exit_codes: vec![],
                    },
                    config: None,
                }),
            )]
            .iter()
            .cloned()
            .collect(),
        };

        assert_eq!(actual.agents.len(), 1);
        assert_eq!(actual, expected);
    }
}
