use super::health_config::OnHostHealthConfig;
use super::templateable_value::TemplateableValue;
use super::version_config::OnHostVersionConfig;
use crate::agent_type::definition::{Variables, include_packages_variables};
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::runtime_config::on_host::executable::Executable;
use crate::agent_type::runtime_config::on_host::filesystem::FileSystem;
use crate::agent_type::runtime_config::on_host::package::{Package, PackageID};
use crate::agent_type::runtime_config::on_host::rendered::RenderedPackages;
use crate::agent_type::templates::Templateable;
use serde::{Deserialize, Deserializer};
use std::collections::{HashMap, HashSet};

pub mod executable;
pub mod filesystem;
pub mod package;
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
    #[serde(default)]
    packages: Packages,
}

type Packages = HashMap<PackageID, Package>;

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
        // First, we template the packages to get their rendered versions, this is needed since we have to
        // know their paths to populate the reserved variables (`${sub-agent:package.<id>.dir}`).
        let rendered_packages: RenderedPackages = self
            .packages
            .into_iter()
            .map(|(agent_id, package)| Ok((agent_id, package.template_with(variables)?)))
            .collect::<Result<RenderedPackages, AgentTypeError>>()?;

        // We include in the variables the packages ones.
        let extended_vars = include_packages_variables(variables.clone(), &rendered_packages)?;

        // Continue the templating normally
        Ok(Self::Output {
            executables: self
                .executables
                .into_iter()
                .map(|e| e.template_with(&extended_vars))
                .collect::<Result<Vec<_>, _>>()?,
            enable_file_logging: self.enable_file_logging.template_with(&extended_vars)?,
            health: self.health.template_with(&extended_vars)?,
            version: self
                .version
                .map(|v| v.template_with(&extended_vars))
                .transpose()?,
            filesystem: self.filesystem.template_with(&extended_vars)?,
            packages: rendered_packages,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::agent_type::agent_attributes::AgentAttributes;
    use crate::agent_type::runtime_config::health_config::HealthCheckTimeout;
    use crate::agent_type::runtime_config::on_host::executable::{Args, Env};
    use crate::agent_type::runtime_config::on_host::package::{Download, Oci};
    use crate::agent_type::runtime_config::restart_policy::{
        self, BackoffDelay, BackoffLastRetryInterval, BackoffStrategyConfig, BackoffStrategyType,
        RestartPolicyConfig,
    };
    use crate::agent_type::variable::Variable;
    use crate::agent_type::variable::namespace::Namespace;
    use crate::checkers::health::health_checker::{HealthCheckInterval, InitialDelay};
    use serde_yaml::Number;
    use std::collections::HashMap;
    use std::path::PathBuf;

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

        let pkg = Package {
            package_type: TemplateableValue::from_template("tar.gz".to_string()),
            download: Download {
                oci: Oci {
                    registry: TemplateableValue::from_template("${nr-var:registry}".to_string()),
                    repository: TemplateableValue::from_template(
                        "${nr-var:repository}".to_string(),
                    ),
                    version: TemplateableValue::from_template("${nr-var:version}".to_string()),
                },
            },
        };

        let expected_packages = HashMap::from([
            ("otel-first".to_string(), pkg.clone()),
            ("otel-second".to_string(), pkg),
        ]);
        assert_eq!(on_host.packages, expected_packages)
    }

    #[test]
    fn test_packages_reserved_variable_dir() {
        // Define an OnHost with one package and an executable using the reserved var
        let yaml = r#"
executables:
  - id: test
    path: ${nr-sub:packages.my-pkg.dir}
    args: ""
packages:
  my-pkg:
    type: tar
    download:
      oci:
        registry: my.registry
        repository: my/repo
        version: latest
"#;
        let on_host: OnHost = serde_yaml::from_str(yaml).unwrap();

        // Base variables must include autogenerated dir
        let mut vars: Variables = Variables::new();
        vars.insert(
            Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_GENERATED_DIR),
            Variable::new_final_string_variable("/autogenerated"),
        );
        vars.insert(
            Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_REMOTE_DIR),
            Variable::new_final_string_variable("remote"),
        );
        vars.insert(
            Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_SUB_AGENT_ID),
            Variable::new_final_string_variable("agent-id"),
        );

        let rendered = on_host.template_with(&vars).unwrap();
        let exe = rendered.executables.first().unwrap();
        assert_eq!(
            exe.path,
            PathBuf::from("remote")
                .join("packages")
                .join("agent-id")
                .join("stored_packages")
                .join("my-pkg")
                .join("oci_my_registry__my__repo_latest")
                .to_string_lossy()
                .to_string(),
        );
    }

    #[test]
    fn test_package_reserved_variable_dir_unknown_pkg_errors() {
        // Executable references a package not existing in the config
        let yaml = r#"
executables:
    - { id: test, path: "${nr-sub:packages.nopkgs.dir}/bin/exe", args: "" }
"#;
        let on_host: OnHost = serde_yaml::from_str(yaml).unwrap();

        let mut vars: Variables = Variables::new();
        vars.insert(
            Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_GENERATED_DIR),
            Variable::new_final_string_variable("/tmp/auto/generated"),
        );
        let err = on_host.template_with(&vars).unwrap_err();
        assert!(
            matches!(err, AgentTypeError::MissingTemplateKey(k) if k.contains("nr-sub:packages.nopkgs.dir"))
        );
    }

    #[test]
    fn test_packages_reserved_variable_dir_unknown_id_errors() {
        // Executable references an unknown package id
        let yaml = r#"
executables:
  - id: test
    path: ${nr-sub:packages.unknown.dir}/bin/exe
    args: ""
packages:
  my-pkg:
    type: tar
    download:
      oci:
        registry: my.registry
        repository: my/repo
        version: latest
"#;
        let on_host: OnHost = serde_yaml::from_str(yaml).unwrap();

        let mut vars: Variables = Variables::new();
        vars.insert(
            Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_GENERATED_DIR),
            Variable::new_final_string_variable("/tmp/auto/autogenerated"),
        );
        vars.insert(
            Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_REMOTE_DIR),
            Variable::new_final_string_variable("/tmp/auto"),
        );
        vars.insert(
            Namespace::SubAgent.namespaced_name(AgentAttributes::VARIABLE_SUB_AGENT_ID),
            Variable::new_final_string_variable("agent-id"),
        );

        // Templating should fail due to missing reserved var for unknown id
        let err = on_host.template_with(&vars).unwrap_err();
        match err {
            AgentTypeError::MissingTemplateKey(key) => {
                assert!(key.contains("nr-sub:packages.unknown.dir"));
            }
            _ => panic!("unexpected error {:?}", err),
        }
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
    fn test_default_health_and_package_config_when_omitted() {
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
            packages: Default::default(),
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
packages:
  otel-first:
    type: tar.gz
    download:
      oci:
        registry: ${nr-var:registry}
        repository: ${nr-var:repository}
        version: ${nr-var:version}
  otel-second:
    type: tar.gz
    download:
      oci:
        registry: ${nr-var:registry}
        repository: ${nr-var:repository}
        version: ${nr-var:version}
"#;
}
