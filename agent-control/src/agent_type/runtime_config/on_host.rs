use std::collections::HashSet;

use serde::{Deserialize, Deserializer};

use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::runtime_config::on_host::executable::Executable;
use crate::agent_type::runtime_config::on_host::filesystem::FileSystem;
use crate::agent_type::templates::Templateable;

use super::health_config::OnHostHealthConfig;
use super::templateable_value::TemplateableValue;
use super::version_config::OnHostVersionConfig;

pub mod executable;
pub mod filesystem;
pub mod rendered;

/// The definition for an on-host supervisor.
///
/// It contains the instructions of what are the agent binaries, command-line arguments, the environment variables passed to it and the restart policy of the supervisor.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct OnHost {
    #[serde(deserialize_with = "deserialize_executables", default)]
    executables: Vec<Executable>,
    #[serde(default)]
    enable_file_logging: TemplateableValue<bool>,
    /// Enables and define health checks configuration.
    #[serde(default)]
    health: OnHostHealthConfig,
    /// Enables and define version checks configuration.
    version: Option<OnHostVersionConfig>,
    #[serde(default)]
    filesystem: FileSystem,
}

fn deserialize_executables<'de, D>(deserializer: D) -> Result<Vec<Executable>, D::Error>
where
    D: Deserializer<'de>,
{
    let executables: Vec<Executable> = Deserialize::deserialize(deserializer)?;
    let mut ids = HashSet::new();

    for executable in &executables {
        let id = executable.id.clone();
        if !ids.insert(id.clone()) {
            return Err(serde::de::Error::custom(format!(
                "Duplicate executable ID found: {id}",
            )));
        }
    }

    Ok(executables)
}

impl Templateable for OnHost {
    type Output = rendered::OnHost;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        Ok(Self::Output {
            executables: self
                .executables
                .into_iter()
                .map(|e| e.template_with(variables))
                .collect::<Result<Vec<_>, _>>()?,
            enable_file_logging: self.enable_file_logging.template_with(variables)?,
            health: self.health.template_with(variables)?,
            version: self
                .version
                .map(|v| v.template_with(variables))
                .transpose()?,
            filesystem: self.filesystem.template_with(variables)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::agent_type::runtime_config::health_config::HealthCheckTimeout;
    use crate::agent_type::runtime_config::on_host::executable::{Args, Env};
    use crate::agent_type::runtime_config::restart_policy::{
        self, BackoffDelay, BackoffLastRetryInterval, BackoffStrategyConfig, BackoffStrategyType,
        RestartPolicyConfig,
    };
    use crate::agent_type::variable::Variable;
    use crate::health::health_checker::{HealthCheckInterval, InitialDelay};
    use serde_yaml::Number;
    use std::collections::HashMap;

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
            id: "otelcol".to_string(),
            path: TemplateableValue::from_template("${nr-var:bin}/otelcol".to_string()),
            args: TemplateableValue::from_template(
                "--verbose ${nr-var:deployment.on_host.verbose} --logs ${nr-var:deployment.on_host.log_level}"
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

        let exec_expected = executable::rendered::Executable {
            id: "otelcol".to_string(),
            path: "/etc/otelcol".to_string(),
            args: Args("--verbose true --logs trace".to_string()),
            env: executable::rendered::Env::default(),
            restart_policy: restart_policy::rendered::RestartPolicyConfig {
                backoff_strategy: restart_policy::rendered::BackoffStrategyConfig {
                    backoff_type: BackoffStrategyType::Exponential,
                    backoff_delay: BackoffDelay::from_secs(10),
                    max_retries: 30.into(),
                    last_retry_interval: BackoffLastRetryInterval::from_secs(300),
                },
            },
        };

        assert_eq!(exec_actual, exec_expected);
    }

    #[test]
    fn test_replacer_two_same() {
        let exec = Executable {
            id: "otelcol".to_string(),
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

        let exec_expected = executable::rendered::Executable {
            id: "otelcol".to_string(),
            path: "/etc/otelcol".to_string(),
            args: Args("--verbose true --verbose_again true".to_string()),
            env: executable::rendered::Env::default(),
            restart_policy: restart_policy::rendered::RestartPolicyConfig {
                backoff_strategy: restart_policy::rendered::BackoffStrategyConfig {
                    backoff_type: BackoffStrategyType::Linear,
                    backoff_delay: BackoffDelay::from_secs(10),
                    max_retries: 30.into(),
                    last_retry_interval: BackoffLastRetryInterval::from_secs(300),
                },
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
        ]);

        let input = Executable {
            id: "myapp".to_string(),
            path: TemplateableValue::from_template("${nr-var:path}".to_string()),
            args: TemplateableValue::from_template("${nr-var:args}".to_string()),
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
            },
        };
        let expected_output = executable::rendered::Executable {
            id: "myapp".to_string(),
            path: "/usr/bin/myapp".to_string(),
            args: Args("--config /etc/myapp.conf".to_string()),
            env: executable::rendered::Env(HashMap::from([(
                "MYAPP_PORT".to_string(),
                "8080".to_string(),
            )])),
            restart_policy: restart_policy::rendered::RestartPolicyConfig {
                backoff_strategy: restart_policy::rendered::BackoffStrategyConfig {
                    backoff_type: BackoffStrategyType::Linear,
                    backoff_delay: BackoffDelay::from_secs(10),
                    max_retries: 30.into(),
                    last_retry_interval: BackoffLastRetryInterval::from_secs(300),
                },
            },
        };
        let actual_output = input.template_with(&variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }

    #[test]
    fn test_default_health_config_when_omitted() {
        let yaml_without_health = r#"
executables:
  - id: otelcol
    path: ${nr-var:bin}/otelcol
    args: "-c ${nr-var:deployment.k8s.image}"
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay: 1s
        max_retries: 3
        last_retry_interval: 30s
"#;

        let on_host: OnHost = serde_yaml::from_str(yaml_without_health).unwrap();

        // If no health is specified the default should be ExecHealth with default values
        let default_health_config = OnHostHealthConfig {
            interval: HealthCheckInterval::default(),
            initial_delay: InitialDelay::default(),
            timeout: HealthCheckTimeout::default(),
            check: None,
        };

        // Create a default OnHost instance to compare
        let default_on_host = OnHost {
            executables: vec![Executable {
                id: "otelcol".to_string(),
                path: TemplateableValue::from_template("${nr-var:bin}/otelcol".to_string()),
                args: TemplateableValue::from_template(
                    "-c ${nr-var:deployment.k8s.image}".to_string(),
                ),
                restart_policy: RestartPolicyConfig {
                    backoff_strategy: BackoffStrategyConfig {
                        backoff_type: TemplateableValue::from_template("fixed".to_string()),
                        backoff_delay: TemplateableValue::from_template("1s".to_string()),
                        max_retries: TemplateableValue::from_template("3".to_string()),
                        last_retry_interval: TemplateableValue::from_template("30s".to_string()),
                    },
                },
                env: Env::default(),
            }],
            enable_file_logging: TemplateableValue::default(),
            health: default_health_config,
            version: None,
            filesystem: FileSystem::default(),
        };

        // Compare the default OnHost instance with the parsed instance
        assert_eq!(on_host, default_on_host);
    }

    #[test]
    fn test_default_fail_if_two_exec_same_id() {
        let yaml_without_health = r#"
executables:
  - id: otelcol
    path: ${nr-var:bin}/otelcol
    args: "-c ${nr-var:deployment.k8s.image}"
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay: 1s
        max_retries: 3
        last_retry_interval: 30s
  - id: otelcol
    path: ${nr-var:bin}/otelcol
    args: "-c ${nr-var:deployment.k8s.image}"
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay: 1s
        max_retries: 3
        last_retry_interval: 30s
"#;

        let on_host = serde_yaml::from_str::<OnHost>(yaml_without_health);

        assert!(on_host.is_err());
        assert!(
            on_host
                .unwrap_err()
                .to_string()
                .contains("Duplicate executable ID found: otelcol")
        );
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
  - id: otelcol
    path: ${nr-var:bin}/otelcol
    args: "-c ${nr-var:deployment.k8s.image}"
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay: 1s
        max_retries: 3
        last_retry_interval: 30s
  - id: otelcol-second
    path: ${nr-var:bin}/otelcol-second
    args: "-c ${nr-var:deployment.k8s.image}"
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay: 1s
        max_retries: 3
        last_retry_interval: 30s
"#;
}
