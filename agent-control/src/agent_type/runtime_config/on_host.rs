use serde::Deserialize;

use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::runtime_config::on_host::executable::Executable;
use crate::agent_type::runtime_config::on_host::filesystem::FileSystem;
use crate::agent_type::templates::Templateable;

use super::health_config::OnHostHealthConfig;
use super::templateable_value::TemplateableValue;

pub mod executable;
pub mod filesystem;

/// The definition for an on-host supervisor.
///
/// It contains the instructions of what are the agent binaries, command-line arguments, the environment variables passed to it and the restart policy of the supervisor.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct OnHost {
    #[serde(default)]
    pub executables: Vec<Executable>,
    #[serde(default)]
    pub enable_file_logging: TemplateableValue<bool>,
    /// Enables and define health checks configuration.
    pub health: Option<OnHostHealthConfig>,
    #[serde(default)]
    pub filesystem: FileSystem,
}

impl Templateable for OnHost {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            executables: self
                .executables
                .into_iter()
                .map(|e| e.template_with(variables))
                .collect::<Result<Vec<_>, _>>()?,
            enable_file_logging: self.enable_file_logging.template_with(variables)?,
            health: self
                .health
                .map(|h| h.template_with(variables))
                .transpose()?,
            filesystem: self.filesystem.template_with(variables)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::agent_type::runtime_config::on_host::executable::{Args, Env};
    use crate::agent_type::runtime_config::restart_policy::{
        BackoffDelay, BackoffLastRetryInterval, BackoffStrategyConfig, BackoffStrategyType,
        RestartPolicyConfig,
    };
    use crate::agent_type::trivial_value::FilePathWithContent;
    use crate::agent_type::variable::Variable;
    use serde_yaml::Number;
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn test_basic_parsing() {
        let on_host: OnHost = serde_yaml::from_str(AGENT_GIVEN_YAML).unwrap();

        assert_eq!(
            "${nr-var:bin}/otelcol",
            on_host.executables.clone().first().unwrap().path.template
        );
        assert_eq!(
            "${nr-var:bin}/otelcol-second",
            on_host.executables.clone().last().unwrap().path.template
        );
        assert_eq!(
            "-c ${nr-var:deployment.k8s.image}".to_string(),
            on_host.executables.clone().first().unwrap().args.template
        );
        assert_eq!(
            "-c ${nr-var:deployment.k8s.image}".to_string(),
            on_host.executables.clone().last().unwrap().args.template
        );
        let backoff_strategy_config = BackoffStrategyConfig {
            backoff_type: TemplateableValue::from_template("fixed".to_string()),
            backoff_delay: TemplateableValue::from_template("1s".to_string()),
            max_retries: TemplateableValue::from_template("3".to_string()),
            last_retry_interval: TemplateableValue::from_template("30s".to_string()),
        };

        // Restart policy values
        assert_eq!(
            backoff_strategy_config,
            on_host
                .executables
                .clone()
                .first()
                .unwrap()
                .restart_policy
                .backoff_strategy
        );
        assert_eq!(
            backoff_strategy_config,
            on_host
                .executables
                .clone()
                .last()
                .unwrap()
                .restart_policy
                .backoff_strategy
        );
    }

    #[test]
    fn test_agent_parsing_omitted_fields_use_defaults() {
        let restart_policy_omitted_fields_yaml = r#"
restart_policy:
  backoff_strategy:
    type: linear
"#;
        let backoff_strategy: BackoffStrategyConfig =
            serde_yaml::from_str(restart_policy_omitted_fields_yaml).unwrap();

        // Restart policy values
        assert_eq!(BackoffStrategyConfig::default(), backoff_strategy);
    }

    #[test]
    fn test_replacer() {
        let exec = Executable {
            path: TemplateableValue::from_template("${nr-var:bin}/otelcol".to_string()),
            args: TemplateableValue::from_template(
                "--config ${nr-var:config} --plugin_dir ${nr-var:integrations} --verbose ${nr-var:deployment.on_host.verbose} --logs ${nr-var:deployment.on_host.log_level}"
                    .to_string(),
            ),
            env: Env::default(),
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue::from_template("${nr-var:backoff.type}".to_string()),
                    backoff_delay: TemplateableValue::from_template("${nr-var:backoff.delay}".to_string()),
                    max_retries: TemplateableValue::from_template("${nr-var:backoff.retries}".to_string()),
                    last_retry_interval: TemplateableValue::from_template(
                        "${nr-var:backoff.interval}".to_string(),
                    ),
                },
                restart_exit_codes: Vec::default(),
            },
        };

        let normalized_values = HashMap::from([
            (
                "nr-var:bin".to_string(),
                Variable::new_string("binary".to_string(), true, None, Some("/etc".to_string())),
            ),
            (
                "nr-var:config".to_string(),
                Variable::new_with_file_path(
                    "config".to_string(),
                    true,
                    None,
                    Some(FilePathWithContent::new(
                        "config2.yml".into(),
                        "license_key: abc123\nstaging: true\n".to_string(),
                    )),
                    "config_path".into(),
                ),
            ),
            (
                "nr-var:integrations".to_string(),
                Variable::new_with_file_path(
                    "integrations".to_string(),
                    true,
                    None,
                    Some(HashMap::from([
                        (
                            "kafka.yml".to_string(),
                            FilePathWithContent::new(
                                "config2.yml".into(),
                                "license_key: abc123\nstaging: true\n".to_string(),
                            ),
                        ),
                        (
                            "redis.yml".to_string(),
                            FilePathWithContent::new(
                                "config2.yml".into(),
                                "license_key: abc123\nstaging: true\n".to_string(),
                            ),
                        ),
                    ])),
                    "integration_path".into(),
                ),
            ),
            (
                "nr-var:deployment.on_host.verbose".to_string(),
                Variable::new_string(
                    "verbosity".to_string(),
                    true,
                    None,
                    Some("true".to_string()),
                ),
            ),
            (
                "nr-var:deployment.on_host.log_level".to_string(),
                Variable::new_string(
                    "log_level".to_string(),
                    true,
                    None,
                    Some("trace".to_string()),
                ),
            ),
            (
                "nr-var:backoff.type".to_string(),
                Variable::new_string(
                    "backoff_type".to_string(),
                    true,
                    None,
                    Some("exponential".to_string()),
                ),
            ),
            (
                "nr-var:backoff.delay".to_string(),
                Variable::new_string(
                    "backoff_delay".to_string(),
                    true,
                    None,
                    Some("10s".to_string()),
                ),
            ),
            (
                "nr-var:backoff.retries".to_string(),
                Variable::new(
                    "backoff_retries".to_string(),
                    true,
                    None,
                    Some(Number::from(30)),
                ),
            ),
            (
                "nr-var:backoff.interval".to_string(),
                Variable::new_string(
                    "backoff_interval".to_string(),
                    true,
                    None,
                    Some("300s".to_string()),
                ),
            ),
        ]);

        let exec_actual = exec.template_with(&normalized_values).unwrap();

        let exec_expected = Executable {
            path: TemplateableValue {
                value: Some("/etc/otelcol".to_string()),
                template: "${nr-var:bin}/otelcol".to_string(),
            },
            args: TemplateableValue {
                value: Some(Args("--config config_path --plugin_dir integration_path --verbose true --logs trace".to_string())),
                template:
                "--config ${nr-var:config} --plugin_dir ${nr-var:integrations} --verbose ${nr-var:deployment.on_host.verbose} --logs ${nr-var:deployment.on_host.log_level}"
                    .to_string(),
            },
            env: Env::default(),
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue {
                        value: Some(BackoffStrategyType::Exponential),
                        template: "${nr-var:backoff.type}".to_string(),
                    },
                    backoff_delay: TemplateableValue {
                        value: Some(BackoffDelay::from_secs(10)),
                        template: "${nr-var:backoff.delay}".to_string(),
                    },
                    max_retries: TemplateableValue {
                        value: Some(30.into()),
                        template: "${nr-var:backoff.retries}".to_string(),
                    },
                    last_retry_interval: TemplateableValue {
                        value: Some(BackoffLastRetryInterval::from_secs(300)),
                        template: "${nr-var:backoff.interval}".to_string(),
                    },
                },
                restart_exit_codes: vec![],
            },
        };

        assert_eq!(exec_actual, exec_expected);
    }

    #[test]
    fn test_replacer_two_same() {
        let exec = Executable {
            path: TemplateableValue::from_template("${nr-var:bin}/otelcol".to_string()),
            args: TemplateableValue::from_template("--verbose ${nr-var:deployment.on_host.verbose} --verbose_again ${nr-var:deployment.on_host.verbose}".to_string()),
            env: Env::default(),
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue::from_template(
                        "${nr-var:backoff.type}"
                            .to_string(),
                    ),
                    backoff_delay: TemplateableValue::from_template(
                        "${nr-var:backoff.delay}"
                            .to_string(),
                    ),
                    max_retries: TemplateableValue::from_template(
                        "${nr-var:backoff.retries}"
                            .to_string(),
                    ),
                    last_retry_interval: TemplateableValue::from_template(
                        "${nr-var:backoff.interval}"
                            .to_string(),
                    ),
                },
                restart_exit_codes: vec![],
            },
        };

        let normalized_values = HashMap::from([
            (
                "nr-var:bin".to_string(),
                Variable::new_string("binary".to_string(), true, None, Some("/etc".to_string())),
            ),
            (
                "nr-var:deployment.on_host.verbose".to_string(),
                Variable::new_string(
                    "verbosity".to_string(),
                    true,
                    None,
                    Some("true".to_string()),
                ),
            ),
            (
                "nr-var:backoff.type".to_string(),
                Variable::new_string(
                    "backoff_type".to_string(),
                    true,
                    None,
                    Some("linear".to_string()),
                ),
            ),
            (
                "nr-var:backoff.delay".to_string(),
                Variable::new_string(
                    "backoff_delay".to_string(),
                    true,
                    None,
                    Some("10s".to_string()),
                ),
            ),
            (
                "nr-var:backoff.retries".to_string(),
                Variable::new(
                    "backoff_retries".to_string(),
                    true,
                    None,
                    Some(Number::from(30)),
                ),
            ),
            (
                "nr-var:backoff.interval".to_string(),
                Variable::new_string(
                    "backoff_interval".to_string(),
                    true,
                    None,
                    Some("300s".to_string()),
                ),
            ),
        ]);

        let exec_actual = exec.template_with(&normalized_values).unwrap();

        let exec_expected = Executable {
            path: TemplateableValue { value: Some("/etc/otelcol".to_string()), template: "${nr-var:bin}/otelcol".to_string() },
            args: TemplateableValue { value: Some(Args("--verbose true --verbose_again true".to_string())), template: "--verbose ${nr-var:deployment.on_host.verbose} --verbose_again ${nr-var:deployment.on_host.verbose}".to_string() },
            env: Env::default(),
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue {
                        value: Some(BackoffStrategyType::Linear),
                        template: "${nr-var:backoff.type}".to_string(),
                    },
                    backoff_delay: TemplateableValue {
                        value: Some(BackoffDelay::from_secs(10)),
                        template: "${nr-var:backoff.delay}".to_string(),
                    },
                    max_retries: TemplateableValue {
                        value: Some(30.into()),
                        template: "${nr-var:backoff.retries}".to_string(),
                    },
                    last_retry_interval: TemplateableValue {
                        value: Some(BackoffLastRetryInterval::from_secs(300)),
                        template: "${nr-var:backoff.interval}".to_string(),
                    },
                },
                restart_exit_codes: vec![],
            },
        };

        assert_eq!(exec_actual, exec_expected);
    }

    #[test]
    fn test_template_executable() {
        let variables = Variables::from([
            (
                "nr-var:path".to_string(),
                Variable::new_string(
                    String::default(),
                    true,
                    None,
                    Some("/usr/bin/myapp".to_string()),
                ),
            ),
            (
                "nr-var:args".to_string(),
                Variable::new_string(
                    String::default(),
                    true,
                    None,
                    Some("--config /etc/myapp.conf".to_string()),
                ),
            ),
            (
                "nr-var:env.MYAPP_PORT".to_string(),
                Variable::new_string(String::default(), true, None, Some("8080".to_string())),
            ),
            (
                "nr-var:backoff.type".to_string(),
                Variable::new_string(String::default(), true, None, Some("linear".to_string())),
            ),
            (
                "nr-var:backoff.delay".to_string(),
                Variable::new_string(String::default(), true, None, Some("10s".to_string())),
            ),
            (
                "nr-var:backoff.retries".to_string(),
                Variable::new(String::default(), true, None, Some(Number::from(30))),
            ),
            (
                "nr-var:backoff.interval".to_string(),
                Variable::new_string(String::default(), true, None, Some("300s".to_string())),
            ),
            (
                "nr-var:config".to_string(),
                Variable::new_with_file_path(
                    "config".to_string(),
                    true,
                    None,
                    Some(FilePathWithContent::new(
                        "config2.yml".into(),
                        "license_key: abc123\nstaging: true\n".to_string(),
                    )),
                    "config_path".into(),
                ),
            ),
            (
                "nr-var:integrations".to_string(),
                Variable::new_with_file_path(
                    "integrations".to_string(),
                    true,
                    None,
                    Some(HashMap::from([(
                        "kafka.yml".to_string(),
                        FilePathWithContent::new(
                            "config2.yml".into(),
                            "license_key: abc123\nstaging: true\n".to_string(),
                        ),
                    )])),
                    "integration_path".into(),
                ),
            ),
        ]);

        let input = Executable {
            path: TemplateableValue::from_template("${nr-var:path}".to_string()),
            args: TemplateableValue::from_template(
                "${nr-var:args} ${nr-var:config} ${nr-var:integrations}".to_string(),
            ),
            env: Env(HashMap::from([(
                "MYAPP_PORT".to_string(),
                TemplateableValue::from_template("${nr-var:env.MYAPP_PORT}".to_string()),
            )])),
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue::from_template(
                        "${nr-var:backoff.type}".to_string(),
                    ),
                    backoff_delay: TemplateableValue::from_template(
                        "${nr-var:backoff.delay}".to_string(),
                    ),
                    max_retries: TemplateableValue::from_template(
                        "${nr-var:backoff.retries}".to_string(),
                    ),
                    last_retry_interval: TemplateableValue::from_template(
                        "${nr-var:backoff.interval}".to_string(),
                    ),
                },
                restart_exit_codes: vec![],
            },
        };
        let expected_output = Executable {
            path: TemplateableValue::new("/usr/bin/myapp".to_string())
                .with_template("${nr-var:path}".to_string()),
            args: TemplateableValue::new(Args(
                "--config /etc/myapp.conf config_path integration_path".to_string(),
            ))
            .with_template("${nr-var:args} ${nr-var:config} ${nr-var:integrations}".to_string()),
            env: Env(HashMap::from([(
                "MYAPP_PORT".to_string(),
                TemplateableValue::new("8080".to_string())
                    .with_template("${nr-var:env.MYAPP_PORT}".to_string()),
            )])),
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue::new(BackoffStrategyType::Linear)
                        .with_template("${nr-var:backoff.type}".to_string()),
                    backoff_delay: TemplateableValue::new(BackoffDelay::from_secs(10))
                        .with_template("${nr-var:backoff.delay}".to_string()),
                    max_retries: TemplateableValue::new(30.into())
                        .with_template("${nr-var:backoff.retries}".to_string()),
                    last_retry_interval: TemplateableValue::new(
                        BackoffLastRetryInterval::from_secs(300),
                    )
                    .with_template("${nr-var:backoff.interval}".to_string()),
                },
                restart_exit_codes: vec![],
            },
        };
        let actual_output = input.template_with(&variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }

    pub const AGENT_GIVEN_YAML: &str = r#"
health:
  interval: 3s
  initial_delay: 3s
  timeout: 10s
  http:
    path: /healthz
    port: 8080
executables:
  - path: ${nr-var:bin}/otelcol
    args: "-c ${nr-var:deployment.k8s.image}"
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay: 1s
        max_retries: 3
        last_retry_interval: 30s
  - path: ${nr-var:bin}/otelcol-second
    args: "-c ${nr-var:deployment.k8s.image}"
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay: 1s
        max_retries: 3
        last_retry_interval: 30s
"#;
}
