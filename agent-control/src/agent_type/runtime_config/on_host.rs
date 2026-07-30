//! On-host deployment configuration: executables, filesystem, packages and check settings.
use super::health_config::OnHostHealthConfig;
use super::templateable_value::TemplateableValue;
use crate::agent_type::definition::{Variables, include_packages_variables};
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::runtime_config::on_host::executable::Executable;
use crate::agent_type::runtime_config::on_host::filesystem::{FileSystem, SharedFileSystem};
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
#[derive(Debug, Default, Clone, PartialEq)]
pub struct OnHost {
    executables: Vec<Executable>,
    enable_file_logging: TemplateableValue<bool>,
    /// Enables and defines health checks configuration.
    health: Option<OnHostHealthConfig>,
    filesystem: FileSystem,
    packages: Packages,
    shared_filesystem: SharedFileSystem,
    /// Package whose OCI version is reported as the `agent.version` identifying attribute.
    reported_version_package: Option<PackageID>,
}

type Packages = HashMap<PackageID, Package>;

impl<'de> Deserialize<'de> for OnHost {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct OnHostRaw {
            #[serde(deserialize_with = "deserialize_executables", default)]
            executables: Vec<Executable>,
            #[serde(default)]
            enable_file_logging: TemplateableValue<bool>,
            #[serde(default)]
            health: Option<OnHostHealthConfig>,
            #[serde(default)]
            filesystem: FileSystem,
            #[serde(default)]
            shared_filesystem: SharedFileSystem,
            #[serde(default)]
            packages: Packages,
            #[serde(default)]
            reported_version_package: Option<PackageID>,
        }

        let raw = OnHostRaw::deserialize(deserializer)?;
        let reported_version_package =
            resolve_reported_version_package(&raw.packages, raw.reported_version_package)?;
        Ok(OnHost {
            executables: raw.executables,
            enable_file_logging: raw.enable_file_logging,
            health: raw.health,
            filesystem: raw.filesystem,
            shared_filesystem: raw.shared_filesystem,
            packages: raw.packages,
            reported_version_package,
        })
    }
}

/// Resolves which package's OCI version is reported as `agent.version`:
/// - an explicit `reported_version_package` must reference a declared package;
/// - with no packages declared, there is nothing to report (`None`);
/// - with exactly one package, it defaults to that package;
/// - with more than one package, `reported_version_package` is required.
fn resolve_reported_version_package<E: serde::de::Error>(
    packages: &Packages,
    declared: Option<PackageID>,
) -> Result<Option<PackageID>, E> {
    if let Some(id) = declared {
        if packages.contains_key(&id) {
            return Ok(Some(id));
        }
        return Err(E::custom(format!(
            "`reported_version_package` references unknown package `{id}`; declared packages: [{}]",
            packages.keys().cloned().collect::<Vec<_>>().join(", ")
        )));
    }

    match packages.len() {
        0 => Ok(None),
        1 => Ok(packages.keys().next().cloned()),
        _ => Err(E::custom(format!(
            "`reported_version_package` is required when more than one package is defined; declared packages: [{}]",
            packages.keys().cloned().collect::<Vec<_>>().join(", ")
        ))),
    }
}

impl OnHost {
    /// The per-agent filesystem entries this Agent Type declares. The paths (keys) are static,
    /// so they are available without rendering.
    pub fn filesystem(&self) -> &FileSystem {
        &self.filesystem
    }

    /// The files and directories this Agent Type declares in the shared filesystem.
    /// The paths are static because they are not [Templateable], therefore they are
    /// available without rendering.
    pub fn shared_filesystem(&self) -> &SharedFileSystem {
        &self.shared_filesystem
    }
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
            health: self
                .health
                .map(|health| health.template_with(&extended_vars))
                .transpose()?,
            filesystem: self.filesystem.template_with(&extended_vars)?,
            shared_filesystem: self.shared_filesystem.template_with(&extended_vars)?,
            packages: rendered_packages,
            reported_version_package: self.reported_version_package,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::agent_type::agent_attributes::AgentAttributes;
    use crate::agent_type::runtime_config::on_host::executable::{Args, Env};
    use crate::agent_type::runtime_config::on_host::package::{Download, Oci};
    use crate::agent_type::runtime_config::restart_policy::{
        self, BackoffDelay, BackoffLastRetryInterval, BackoffStrategyConfig, BackoffStrategyType,
        RestartPolicyConfig,
    };
    use crate::agent_type::variable::Variable;
    use crate::agent_type::variable::namespace::{Namespace, NamespacedVariableName};
    use serde_json::Number;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn test_basic_parsing() {
        let on_host: OnHost = serde_saphyr::from_str(AGENT_GIVEN_YAML).unwrap();

        let args = on_host.executables.clone().first().unwrap().args.0.clone();
        assert_eq!(
            "${nr-var:bin}/otelcol",
            on_host.executables.clone().first().unwrap().path.template
        );
        assert_eq!(
            "${nr-var:bin}/otelcol-second",
            on_host.executables.clone().last().unwrap().path.template
        );
        assert_eq!("-c".to_string(), args.first().unwrap().template);
        assert_eq!(
            "${nr-var:deployment.k8s.image}".to_string(),
            args.last().unwrap().template
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
            download: Download {
                oci: Oci {
                    repository: TemplateableValue::from_template(
                        "${nr-var:repository}".to_string(),
                    ),
                    version: TemplateableValue::from_template("${nr-var:version}".to_string()),
                    public_key_url: Some(TemplateableValue::from_template(
                        "${nr-var:public-key-url}".to_string(),
                    )),
                },
            },
            post_download_hook: None,
        };

        let expected_packages = HashMap::from([
            ("otel-first".to_string(), pkg.clone()),
            ("otel-second".to_string(), pkg),
        ]);
        assert_eq!(on_host.packages, expected_packages)
    }

    #[test]
    fn test_packages_reserved_variable_dir_and_no_public_key_url() {
        // Define an OnHost with one package and an executable using the reserved var
        let yaml = r#"
executables:
  - id: test
    path: ${nr-sub:packages.my-pkg.dir}
    args: []
packages:
  my-pkg:
    download:
      oci:
        repository: my/repo
        version: latest
"#;
        let on_host: OnHost = serde_saphyr::from_str(yaml).unwrap();

        // Base variables must include autogenerated dir
        let mut vars: Variables = Variables::new();
        vars.insert(
            NamespacedVariableName::new(
                Namespace::SubAgent,
                AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR,
            ),
            Variable::new_final_string_variable("/filesystem"),
        );
        vars.insert(
            NamespacedVariableName::new(Namespace::SubAgent, AgentAttributes::VARIABLE_REMOTE_DIR),
            Variable::new_final_string_variable("remote"),
        );
        vars.insert(
            NamespacedVariableName::new(
                Namespace::SubAgent,
                AgentAttributes::VARIABLE_SUB_AGENT_ID,
            ),
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
                .join("oci_my__repo_latest")
                .to_string_lossy()
                .to_string(),
        );
    }

    #[test]
    fn reported_version_package_defaults_to_sole_package() {
        let yaml = r#"
packages:
  infra:
    download:
      oci:
        repository: newrelic-infra
        version: "1.2.3"
"#;
        let on_host: OnHost = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(on_host.reported_version_package, Some("infra".to_string()));
    }

    #[test]
    fn reported_version_package_explicit_selection_with_multiple_packages() {
        let yaml = r#"
reported_version_package: infra
packages:
  infra:
    download:
      oci:
        repository: newrelic-infra
        version: "1.2.3"
  flex:
    download:
      oci:
        repository: nri-flex
        version: "4.5.6"
"#;
        let on_host: OnHost = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(on_host.reported_version_package, Some("infra".to_string()));
    }

    #[test]
    fn reported_version_package_required_when_multiple_packages() {
        let yaml = r#"
packages:
  infra:
    download:
      oci:
        repository: newrelic-infra
        version: "1.2.3"
  flex:
    download:
      oci:
        repository: nri-flex
        version: "4.5.6"
"#;
        let err = serde_saphyr::from_str::<OnHost>(yaml)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("reported_version_package") && err.contains("required"),
            "unexpected error: {err}"
        );
        assert!(
            err.contains("flex") && err.contains("infra"),
            "error should list declared package ids: {err}"
        );
    }

    #[test]
    fn reported_version_package_referencing_unknown_id_errors() {
        let yaml = r#"
reported_version_package: does-not-exist
packages:
  infra:
    download:
      oci:
        repository: newrelic-infra
        version: "1.2.3"
"#;
        let err = serde_saphyr::from_str::<OnHost>(yaml)
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown package"), "unexpected error: {err}");
    }

    #[test]
    fn reported_version_package_set_without_packages_errors() {
        let yaml = "reported_version_package: infra\n";
        let err = serde_saphyr::from_str::<OnHost>(yaml)
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown package"), "unexpected error: {err}");
    }

    #[test]
    fn no_packages_yields_no_reported_version_package() {
        let yaml = r#"
executables:
  - id: test
    path: /bin/true
    args: []
"#;
        let on_host: OnHost = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(on_host.reported_version_package, None);
    }

    #[test]
    fn test_package_reserved_variable_dir_unknown_pkg_errors() {
        // Executable references a package not existing in the config
        let yaml = r#"
executables:
    - { id: test, path: "${nr-sub:packages.nopkgs.dir}/bin/exe", args: [] }
"#;
        let on_host: OnHost = serde_saphyr::from_str(yaml).unwrap();

        let mut vars: Variables = Variables::new();
        vars.insert(
            NamespacedVariableName::new(
                Namespace::SubAgent,
                AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR,
            ),
            Variable::new_final_string_variable("/tmp/auto/filesystem"),
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
    args: []
packages:
  my-pkg:
    download:
      oci:
        registry: my.registry
        repository: my/repo
        version: latest
"#;
        let on_host: OnHost = serde_saphyr::from_str(yaml).unwrap();

        let mut vars: Variables = Variables::new();
        vars.insert(
            NamespacedVariableName::new(
                Namespace::SubAgent,
                AgentAttributes::VARIABLE_FILESYSTEM_AGENT_DIR,
            ),
            Variable::new_final_string_variable("/tmp/auto/filesystem"),
        );
        vars.insert(
            NamespacedVariableName::new(Namespace::SubAgent, AgentAttributes::VARIABLE_REMOTE_DIR),
            Variable::new_final_string_variable("/tmp/auto"),
        );
        vars.insert(
            NamespacedVariableName::new(
                Namespace::SubAgent,
                AgentAttributes::VARIABLE_SUB_AGENT_ID,
            ),
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
            serde_saphyr::from_str(restart_policy_omitted_fields_yaml).unwrap();

        // Restart policy values
        assert_eq!(BackoffStrategyConfig::default(), backoff_strategy);
    }

    #[test]
    fn test_replacer() {
        let exec = Executable {
            id: "otelcol".to_string(),
            path: TemplateableValue::from_template("${nr-var:bin}/otelcol".to_string()),
            args: Args(vec![
                TemplateableValue::from_template("--verbose".to_string()),
                TemplateableValue::from_template(
                    "${nr-var:deployment.on_host.verbose}".to_string(),
                ),
                TemplateableValue::from_template("--logs".to_string()),
                TemplateableValue::from_template(
                    "${nr-var:deployment.on_host.log_level}".to_string(),
                ),
            ]),
            env: Env::default(),
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

        let normalized_values = HashMap::from([
            (
                NamespacedVariableName::new(Namespace::Variable, "bin"),
                Variable::new_string("binary".to_string(), true, None, Some("/etc".to_string())),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "deployment.on_host.verbose"),
                Variable::new_string(
                    "verbosity".to_string(),
                    true,
                    None,
                    Some("true".to_string()),
                ),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "deployment.on_host.log_level"),
                Variable::new_string(
                    "log_level".to_string(),
                    true,
                    None,
                    Some("trace".to_string()),
                ),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "backoff.type"),
                Variable::new_string(
                    "backoff_type".to_string(),
                    true,
                    None,
                    Some("exponential".to_string()),
                ),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "backoff.delay"),
                Variable::new_string(
                    "backoff_delay".to_string(),
                    true,
                    None,
                    Some("10s".to_string()),
                ),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "backoff.retries"),
                Variable::new(
                    "backoff_retries".to_string(),
                    true,
                    None,
                    Some(Number::from(30)),
                ),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "backoff.interval"),
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
            args: executable::rendered::Args(vec![
                "--verbose".to_string(),
                "true".to_string(),
                "--logs".to_string(),
                "trace".to_string(),
            ]),
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
            args: Args(vec![
                TemplateableValue::from_template("--verbose".to_string()),
                TemplateableValue::from_template(
                    "${nr-var:deployment.on_host.verbose}".to_string(),
                ),
                TemplateableValue::from_template("--verbose_again".to_string()),
                TemplateableValue::from_template(
                    "${nr-var:deployment.on_host.verbose}".to_string(),
                ),
            ]),
            env: Env::default(),
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

        let normalized_values = HashMap::from([
            (
                NamespacedVariableName::new(Namespace::Variable, "bin"),
                Variable::new_string("binary".to_string(), true, None, Some("/etc".to_string())),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "deployment.on_host.verbose"),
                Variable::new_string(
                    "verbosity".to_string(),
                    true,
                    None,
                    Some("true".to_string()),
                ),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "backoff.type"),
                Variable::new_string(
                    "backoff_type".to_string(),
                    true,
                    None,
                    Some("linear".to_string()),
                ),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "backoff.delay"),
                Variable::new_string(
                    "backoff_delay".to_string(),
                    true,
                    None,
                    Some("10s".to_string()),
                ),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "backoff.retries"),
                Variable::new(
                    "backoff_retries".to_string(),
                    true,
                    None,
                    Some(Number::from(30)),
                ),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "backoff.interval"),
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
            args: executable::rendered::Args(vec![
                "--verbose".to_string(),
                "true".to_string(),
                "--verbose_again".to_string(),
                "true".to_string(),
            ]),
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
                NamespacedVariableName::new(Namespace::Variable, "path"),
                Variable::new_string(
                    String::default(),
                    true,
                    None,
                    Some("/usr/bin/myapp".to_string()),
                ),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "args"),
                Variable::new_string(String::default(), true, None, Some("--my_arg".to_string())),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "env.MYAPP_PORT"),
                Variable::new_string(String::default(), true, None, Some("8080".to_string())),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "backoff.type"),
                Variable::new_string(String::default(), true, None, Some("linear".to_string())),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "backoff.delay"),
                Variable::new_string(String::default(), true, None, Some("10s".to_string())),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "backoff.retries"),
                Variable::new(String::default(), true, None, Some(Number::from(30))),
            ),
            (
                NamespacedVariableName::new(Namespace::Variable, "backoff.interval"),
                Variable::new_string(String::default(), true, None, Some("300s".to_string())),
            ),
        ]);

        let input = Executable {
            id: "myapp".to_string(),
            path: TemplateableValue::from_template("${nr-var:path}".to_string()),
            args: Args(vec![TemplateableValue::from_template(
                "${nr-var:args}".to_string(),
            )]),
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
            args: executable::rendered::Args(vec!["--my_arg".to_string()]),
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
    args:
      - -c
      - ${nr-var:deployment.k8s.image}
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay: 1s
        max_retries: 3
        last_retry_interval: 30s
"#;

        let on_host: OnHost = serde_saphyr::from_str(yaml_without_health).unwrap();

        // When `health:` is omitted, the parsed value should be `None` (no health checker
        // will be spawned by the supervisor).
        let default_on_host = OnHost {
            executables: vec![Executable {
                id: "otelcol".to_string(),
                path: TemplateableValue::from_template("${nr-var:bin}/otelcol".to_string()),
                args: Args(vec![
                    TemplateableValue::from_template("-c".to_string()),
                    TemplateableValue::from_template("${nr-var:deployment.k8s.image}".to_string()),
                ]),
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
            health: None,
            filesystem: FileSystem::default(),
            shared_filesystem: SharedFileSystem::default(),
            packages: Default::default(),
            reported_version_package: None,
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
    args:
      - -c
      - ${nr-var:deployment.k8s.image}
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay: 1s
        max_retries: 3
        last_retry_interval: 30s
  - id: otelcol
    path: ${nr-var:bin}/otelcol
    args:
      - -c
      - ${nr-var:deployment.k8s.image}
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay: 1s
        max_retries: 3
        last_retry_interval: 30s
"#;

        let on_host = serde_saphyr::from_str::<OnHost>(yaml_without_health);

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
  checks:
    - kind: Process
    - kind: Http
      path: /healthz
      port: 8080
executables:
  - id: otelcol
    path: ${nr-var:bin}/otelcol
    args:
      - -c
      - ${nr-var:deployment.k8s.image}
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay: 1s
        max_retries: 3
        last_retry_interval: 30s
  - id: otelcol-second
    path: ${nr-var:bin}/otelcol-second
    args:
      - -c
      - ${nr-var:deployment.k8s.image}
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay: 1s
        max_retries: 3
        last_retry_interval: 30s
reported_version_package: otel-first
packages:
  otel-first:
    download:
      oci:
        registry: ${nr-var:registry}
        repository: ${nr-var:repository}
        version: ${nr-var:version}
        public_key_url: ${nr-var:public-key-url}
  otel-second:
    download:
      oci:
        registry: ${nr-var:registry}
        repository: ${nr-var:repository}
        version: ${nr-var:version}
        public_key_url: ${nr-var:public-key-url}
"#;
}
