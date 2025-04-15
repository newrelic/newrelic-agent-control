use std::collections::HashMap;

use serde::Deserialize;

use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::templates::Templateable;

use super::health_config::OnHostHealthConfig;
use super::restart_policy::RestartPolicyConfig;
use super::templateable_value::TemplateableValue;

/// The definition for an on-host supervisor.
///
/// It contains the instructions of what are the agent binaries, command-line arguments, the environment variables passed to it and the restart policy of the supervisor.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct OnHost {
    pub executable: Option<Executable>,
    #[serde(default)]
    pub enable_file_logging: TemplateableValue<bool>,
    /// Enables and define health checks configuration.
    pub health: Option<OnHostHealthConfig>,
}

impl Templateable for OnHost {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            executable: self
                .executable
                .map(|e| e.template_with(variables))
                .transpose()?,
            enable_file_logging: self.enable_file_logging.template_with(variables)?,
            health: self
                .health
                .map(|h| h.template_with(variables))
                .transpose()?,
        })
    }
}

/* FIXME: This is not TEMPLATEABLE for the moment, we need to think what would be the strategy here and clarify:

1. If we perform replacement with the template but the values are not of the expected type, what happens?
2. Should we use an intermediate type with all the end nodes as `String` so we can perform the replacement?
- Add a sanitize or a fallible conversion from the raw intermediate type into into the end type?
*/
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct Executable {
    /// Executable binary path. If not an absolute path, the PATH will be searched in an OS-defined way.
    pub path: TemplateableValue<String>, // make it templatable

    /// Arguments passed to the executable.
    #[serde(default)]
    pub args: TemplateableValue<Args>, // make it templatable, it should be aware of the value type, if templated with array, should be expanded

    /// Environmental variables passed to the process.
    #[serde(default)]
    pub env: Env,

    /// Defines how the executable will be restarted in case of failure.
    #[serde(default)]
    pub restart_policy: RestartPolicyConfig,
}

impl Templateable for Executable {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            path: self.path.template_with(variables)?,
            args: self.args.template_with(variables)?,
            env: self.env.template_with(variables)?,
            restart_policy: self.restart_policy.template_with(variables)?,
        })
    }
}

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct Args(pub String);

impl Args {
    pub fn into_vector(self) -> Vec<String> {
        self.0.split_whitespace().map(|s| s.to_string()).collect()
    }
}

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct Env(pub(super) HashMap<String, TemplateableValue<String>>);

impl Env {
    pub fn get(self) -> HashMap<String, String> {
        self.0.into_iter().map(|(k, v)| (k, v.get())).collect()
    }
}

impl Templateable for Env {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        self.0
            .into_iter()
            .map(|(k, v)| Ok((k, v.template_with(variables)?)))
            .collect::<Result<HashMap<_, _>, _>>()
            .map(Env)
    }
}

#[cfg(test)]
mod tests {
    use crate::agent_type::runtime_config::restart_policy::{
        BackoffDelay, BackoffLastRetryInterval, BackoffStrategyConfig, BackoffStrategyType,
    };
    use crate::agent_type::trivial_value::FilePathWithContent;
    use crate::agent_type::variable::definition::VariableDefinition;
    use serde_yaml::Number;
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn test_basic_parsing() {
        let on_host: OnHost = serde_yaml::from_str(AGENT_GIVEN_YAML).unwrap();

        assert_eq!(
            "${nr-var:bin}/otelcol",
            on_host.executable.clone().unwrap().path.template
        );
        assert_eq!(
            "-c ${nr-var:deployment.k8s.image}".to_string(),
            on_host.executable.clone().unwrap().args.template
        );

        // Restart policy values
        assert_eq!(
            BackoffStrategyConfig {
                backoff_type: TemplateableValue::from_template("fixed".to_string()),
                backoff_delay: TemplateableValue::from_template("1s".to_string()),
                max_retries: TemplateableValue::from_template("3".to_string()),
                last_retry_interval: TemplateableValue::from_template("30s".to_string()),
            },
            on_host.executable.unwrap().restart_policy.backoff_strategy
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
                VariableDefinition::new("binary".to_string(), true, None, Some("/etc".to_string())),
            ),
            (
                "nr-var:config".to_string(),
                VariableDefinition::new_with_file_path(
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
                VariableDefinition::new_with_file_path(
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
                VariableDefinition::new(
                    "verbosity".to_string(),
                    true,
                    None,
                    Some("true".to_string()),
                ),
            ),
            (
                "nr-var:deployment.on_host.log_level".to_string(),
                VariableDefinition::new(
                    "log_level".to_string(),
                    true,
                    None,
                    Some("trace".to_string()),
                ),
            ),
            (
                "nr-var:backoff.type".to_string(),
                VariableDefinition::new(
                    "backoff_type".to_string(),
                    true,
                    None,
                    Some("exponential".to_string()),
                ),
            ),
            (
                "nr-var:backoff.delay".to_string(),
                VariableDefinition::new(
                    "backoff_delay".to_string(),
                    true,
                    None,
                    Some("10s".to_string()),
                ),
            ),
            (
                "nr-var:backoff.retries".to_string(),
                VariableDefinition::new(
                    "backoff_retries".to_string(),
                    true,
                    None,
                    Some(Number::from(30)),
                ),
            ),
            (
                "nr-var:backoff.interval".to_string(),
                VariableDefinition::new(
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
                VariableDefinition::new("binary".to_string(), true, None, Some("/etc".to_string())),
            ),
            (
                "nr-var:deployment.on_host.verbose".to_string(),
                VariableDefinition::new(
                    "verbosity".to_string(),
                    true,
                    None,
                    Some("true".to_string()),
                ),
            ),
            (
                "nr-var:backoff.type".to_string(),
                VariableDefinition::new(
                    "backoff_type".to_string(),
                    true,
                    None,
                    Some("linear".to_string()),
                ),
            ),
            (
                "nr-var:backoff.delay".to_string(),
                VariableDefinition::new(
                    "backoff_delay".to_string(),
                    true,
                    None,
                    Some("10s".to_string()),
                ),
            ),
            (
                "nr-var:backoff.retries".to_string(),
                VariableDefinition::new(
                    "backoff_retries".to_string(),
                    true,
                    None,
                    Some(Number::from(30)),
                ),
            ),
            (
                "nr-var:backoff.interval".to_string(),
                VariableDefinition::new(
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
                VariableDefinition::new(
                    String::default(),
                    true,
                    None,
                    Some("/usr/bin/myapp".to_string()),
                ),
            ),
            (
                "nr-var:args".to_string(),
                VariableDefinition::new(
                    String::default(),
                    true,
                    None,
                    Some("--config /etc/myapp.conf".to_string()),
                ),
            ),
            (
                "nr-var:env.MYAPP_PORT".to_string(),
                VariableDefinition::new(String::default(), true, None, Some("8080".to_string())),
            ),
            (
                "nr-var:backoff.type".to_string(),
                VariableDefinition::new(String::default(), true, None, Some("linear".to_string())),
            ),
            (
                "nr-var:backoff.delay".to_string(),
                VariableDefinition::new(String::default(), true, None, Some("10s".to_string())),
            ),
            (
                "nr-var:backoff.retries".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(Number::from(30))),
            ),
            (
                "nr-var:backoff.interval".to_string(),
                VariableDefinition::new(String::default(), true, None, Some("300s".to_string())),
            ),
            (
                "nr-var:config".to_string(),
                VariableDefinition::new_with_file_path(
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
                VariableDefinition::new_with_file_path(
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
  timeout: 10s
  http:
    path: /healthz
    port: 8080
executable:
  path: ${nr-var:bin}/otelcol
  args: "-c ${nr-var:deployment.k8s.image}"
  restart_policy:
    backoff_strategy:
      type: fixed
      backoff_delay: 1s
      max_retries: 3
      last_retry_interval: 30s
"#;
}
