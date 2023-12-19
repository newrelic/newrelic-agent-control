use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use tracing::warn;

use super::{
    agent_types::{EndSpec, NormalizedVariables, VariableType},
    error::AgentTypeError,
    restart_policy::{BackoffStrategyConfig, RestartPolicyConfig},
    runtime_config::{Deployment, Executable, K8s, K8sObject, OnHost, RuntimeConfig},
};

/// Regex that extracts the template values from a string.
///
/// Example:
///
/// ```
/// use regex::Regex;
///
/// const TEMPLATE_RE: &str = r"\$\{([a-zA-Z0-9\.\-_/]+)\}";
/// let re = Regex::new(TEMPLATE_RE).unwrap();
/// let content = "Hello ${name.value}!";
///
/// let result = re.find_iter(content).map(|i| i.as_str()).collect::<Vec<_>>();
///
/// assert_eq!(result, vec!["${name.value}"]);
const TEMPLATE_RE: &str = r"\$\{([a-zA-Z0-9\.\-_/]+)\}";
const TEMPLATE_BEGIN: &str = "${";
const TEMPLATE_END: char = '}';
pub const TEMPLATE_KEY_SEPARATOR: &str = ".";

fn template_re() -> &'static Regex {
    static RE_ONCE: OnceLock<Regex> = OnceLock::new();
    RE_ONCE.get_or_init(|| Regex::new(TEMPLATE_RE).unwrap())
}

fn only_template_var_re() -> &'static Regex {
    static ONLY_RE_ONCE: OnceLock<Regex> = OnceLock::new();
    ONLY_RE_ONCE.get_or_init(|| Regex::new(format!("^{TEMPLATE_RE}$").as_str()).unwrap())
}

/// Returns a string slice with the template's begin and end trimmed.
fn template_trim(s: &str) -> &str {
    s.trim_start_matches(TEMPLATE_BEGIN)
        .trim_end_matches(TEMPLATE_END)
}

/// Returns a variable reference from the provided set if it exists, it returns an error otherwise.
fn normalized_var<'a>(
    name: &str,
    variables: &'a NormalizedVariables,
) -> Result<&'a EndSpec, AgentTypeError> {
    variables
        .get(name)
        .ok_or(AgentTypeError::MissingTemplateKey(name.to_string()))
}

/// Returns a string with the first match of a variable replaced with the corresponding value
/// (according to the provided normalized variable).
fn replace(
    re: &Regex,
    s: &str,
    var_name: &str,
    normalized_var: &EndSpec,
) -> Result<String, AgentTypeError> {
    let value = normalized_var
        .get_template_value()
        .ok_or(AgentTypeError::MissingTemplateKey(var_name.to_string()))?
        .to_string();

    Ok(re.replace(s, value).to_string())
}

pub trait Templateable {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError>
    where
        Self: std::marker::Sized;
}

impl Templateable for Executable {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            path: self.path.template_with(variables)?,
            args: self.args.template_with(variables)?,
            env: self.env.template_with(variables)?,
            restart_policy: self.restart_policy.template_with(variables)?,
        })
    }
}

// The actual std type that has a meaningful implementation of Templateable
impl Templateable for String {
    fn template_with(self, variables: &NormalizedVariables) -> Result<String, AgentTypeError> {
        template_string(self, variables)
    }
}

fn template_string(s: String, variables: &NormalizedVariables) -> Result<String, AgentTypeError> {
    let re = template_re();
    re.find_iter(&s)
        .map(|i| i.as_str())
        .try_fold(s.clone(), |r, i| {
            let var_name = template_trim(i);
            replace(re, &r, var_name, normalized_var(var_name, variables)?)
        })
}

impl Templateable for OnHost {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            executables: self
                .executables
                .into_iter()
                .map(|e| e.template_with(variables))
                .collect::<Result<Vec<Executable>, AgentTypeError>>()?,
        })
    }
}

impl Templateable for RestartPolicyConfig {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            backoff_strategy: self.backoff_strategy.template_with(variables)?,
            restart_exit_codes: self.restart_exit_codes, // TODO Not templating this for now!
        })
    }
}

impl Templateable for BackoffStrategyConfig {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        let backoff_type = self.backoff_type.template_with(variables)?;
        let backoff_delay = self.backoff_delay.template_with(variables)?;
        let max_retries = self.max_retries.template_with(variables)?;
        let last_retry_interval = self.last_retry_interval.template_with(variables)?;

        let result = Self {
            backoff_type,
            backoff_delay,
            max_retries,
            last_retry_interval,
        };

        if !result.are_values_in_sync_with_type() {
            warn!("Backoff strategy type is set to `none`, but some of the backoff strategy fields are set. They will be ignored.");
        }

        Ok(result)
    }
}

impl Templateable for K8s {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            objects: self
                .objects
                .into_iter()
                .map(|(k, v)| Ok((k, v.template_with(variables)?)))
                .collect::<Result<HashMap<String, K8sObject>, AgentTypeError>>()?,
        })
    }
}

impl Templateable for K8sObject {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            api_version: self.api_version.clone(),
            kind: self.kind.clone(),
            fields: self.fields.template_with(variables)?,
        })
    }
}

impl Templateable for serde_yaml::Value {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        let templated_value = match self {
            serde_yaml::Value::Mapping(m) => {
                serde_yaml::Value::Mapping(m.template_with(variables)?)
            }
            serde_yaml::Value::Sequence(seq) => {
                serde_yaml::Value::Sequence(seq.template_with(variables)?)
            }
            serde_yaml::Value::String(st) => template_yaml_value_string(st, variables)?,
            _ => self,
        };

        Ok(templated_value)
    }
}

impl Templateable for serde_yaml::Mapping {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        self.into_iter()
            .map(|(k, v)| Ok((k, v.template_with(variables)?)))
            .collect()
    }
}

impl Templateable for serde_yaml::Sequence {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        self.into_iter()
            .map(|v| v.template_with(variables))
            .collect()
    }
}

/// Templates yaml strings as [serde_yaml::Value].
/// When all the string content is a variable template, the corresponding variable type is checked
/// and the value is handled as needed. Otherwise, it is templated as a regular string. Example:
///
/// ```yaml
/// key1: ${var} # The var type is checked and the expanded value might not be a string.
/// # The examples below are always templated as string, regardless of the variable type.
/// key2: this-${var}
/// key3: ${var}${var}
/// ```
fn template_yaml_value_string(
    s: String,
    variables: &NormalizedVariables,
) -> Result<serde_yaml::Value, AgentTypeError> {
    if !only_template_var_re().is_match(s.as_str()) {
        let templated = template_string(s, variables)?;
        return Ok(serde_yaml::Value::String(templated));
    }
    let var_name = template_trim(s.as_str());
    let replacement = normalized_var(var_name, variables)?;
    let replacement_value = replacement
        .get_template_value()
        .ok_or(AgentTypeError::MissingAgentKey(var_name.to_string()))?;
    match replacement.type_ {
        VariableType::Yaml => {
            replacement_value
                .to_yaml_value()
                .ok_or(AgentTypeError::InvalidValueForSpec {
                    key: var_name.to_string(),
                    type_: VariableType::Yaml,
                })
        }
        VariableType::Bool | VariableType::Number => {
            serde_yaml::from_str(replacement_value.to_string().as_str())
                .map_err(AgentTypeError::SerdeYaml)
        }
        _ => Ok(serde_yaml::Value::String(replacement_value.to_string())),
    }
}

impl Templateable for Deployment {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        /*
        `self.on_host` has type `Option<OnHost>`

        let t = self.on_host.map(|o| o.template_with(variables)); `t` has type `Option<Result<OnHost, AgentTypeError>>`

        Let's visit all the possibilities of `t`.
        When I do `t.transpose()`, which takes an Option<Result<_,_>> and returns a Result<Option<_>,_>, this is what happens:

        ```
        match t {
            None => Ok(None),
            Some(Ok(on_host)) => Ok(Some(on_host)),
            Some(Err(e)) => Err(e),
        }
        ```

        In words:
        - None will be mapped to Ok(None).
        - Some(Ok(_)) will be mapped to Ok(Some(_)).
        - Some(Err(_)) will be mapped to Err(_).

        With `?` I get rid of the original Result<_,_> wrapper type and get the Option<_> (or else the error bubbles up if it contained the Err(_) variant). Then I am able to store that Option<_>, be it None or Some(_), back into the Deployment object which contains the Option<_> field.
        */

        let oh = self
            .on_host
            .map(|oh| oh.template_with(variables))
            .transpose()?;
        let k8s = self
            .k8s
            .map(|k8s| k8s.template_with(variables))
            .transpose()?;
        Ok(Self { on_host: oh, k8s })
    }
}

impl Templateable for RuntimeConfig {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            deployment: self.deployment.template_with(variables)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;

    use crate::config::agent_type::restart_policy::{BackoffDuration, BackoffStrategyType};
    use crate::config::agent_type::trivial_value::FilePathWithContent;
    use crate::config::agent_type::trivial_value::N::PosInt;
    use crate::config::agent_type::{
        agent_types::{EndSpec, TemplateableValue, VariableType},
        runtime_config::{Args, Env},
        trivial_value::{TrivialValue, N},
    };
    use std::collections::HashMap;

    use super::*;

    impl EndSpec {
        fn default_with_type(type_: VariableType) -> Self {
            Self {
                type_,
                final_value: None,
                default: None,
                description: String::default(),
                required: false,
                file_path: None,
            }
        }
    }

    #[test]
    fn test_template_string() {
        let variables = NormalizedVariables::from([
            (
                "name".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::String("Alice".to_string())),
                    default: None,
                    type_: VariableType::String,
                    description: String::default(),
                    required: true,
                    file_path: Some("some_path".to_string()),
                },
            ),
            (
                "age".to_string(),
                EndSpec {
                    final_value: None,
                    default: Some(TrivialValue::Number(N::PosInt(30))),
                    type_: VariableType::Number,
                    description: String::default(),
                    required: false,
                    file_path: Some("some_path".to_string()),
                },
            ),
        ]);

        let input = "Hello ${name}! You are ${age} years old.".to_string();
        let expected_output = "Hello Alice! You are 30 years old.".to_string();
        let actual_output = template_string(input, &variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }

    #[test]
    fn test_template_executable() {
        let variables = NormalizedVariables::from([
            (
                "path".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::String("/usr/bin/myapp".to_string())),
                    default: None,
                    description: String::default(),
                    required: true,
                    type_: VariableType::String,
                    file_path: Some("some_path".to_string()),
                },
            ),
            (
                "args".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::String("--config /etc/myapp.conf".to_string())),
                    default: None,
                    description: String::default(),
                    required: true,
                    type_: VariableType::String,
                    file_path: Some("some_path".to_string()),
                },
            ),
            (
                "env.MYAPP_PORT".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Number(N::PosInt(8080))),
                    default: None,
                    description: String::default(),
                    required: true,
                    type_: VariableType::Number,
                    file_path: Some("some_path".to_string()),
                },
            ),
            (
                "backoff.type".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::String("linear".to_string())),
                    default: None,
                    description: "backoff_type".to_string(),
                    type_: VariableType::String,
                    required: true,
                    file_path: Some("some_path".to_string()),
                },
            ),
            (
                "backoff.delay".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::String("10s".to_string())),
                    default: None,
                    description: "backoff_delay".to_string(),
                    type_: VariableType::String,
                    required: true,
                    file_path: Some("some_path".to_string()),
                },
            ),
            (
                "backoff.retries".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Number(PosInt(30))),
                    default: None,
                    description: "backoff_retries".to_string(),
                    type_: VariableType::String,
                    required: true,
                    file_path: Some("some_path".to_string()),
                },
            ),
            (
                "backoff.interval".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::String("300s".to_string())),
                    default: None,
                    description: "backoff_interval".to_string(),
                    type_: VariableType::Number,
                    required: true,
                    file_path: Some("some_path".to_string()),
                },
            ),
            (
                "config".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::File(FilePathWithContent::new(
                        "config2.yml".to_string(),
                        "license_key: abc123\nstaging: true\n".to_string(),
                    ))),
                    default: None,
                    description: "config".to_string(),
                    type_: VariableType::File,
                    required: true,
                    file_path: Some("config_path".to_string()),
                },
            ),
            (
                "integrations".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Map(HashMap::from([(
                        "kafka.yml".to_string(),
                        TrivialValue::File(FilePathWithContent::new(
                            "config2.yml".to_string(),
                            "license_key: abc123\nstaging: true\n".to_string(),
                        )),
                    )]))),
                    default: None,
                    description: "integrations".to_string(),
                    type_: VariableType::MapStringFile,
                    required: true,
                    file_path: Some("integration_path".to_string()),
                },
            ),
        ]);

        let input = Executable {
            path: TemplateableValue::from_template("${path}".to_string()),
            args: TemplateableValue::from_template("${args} ${config} ${integrations}".to_string()),
            env: TemplateableValue::from_template("MYAPP_PORT=${env.MYAPP_PORT}".to_string()),
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue::from_template("${backoff.type}".to_string()),
                    backoff_delay: TemplateableValue::from_template("${backoff.delay}".to_string()),
                    max_retries: TemplateableValue::from_template("${backoff.retries}".to_string()),
                    last_retry_interval: TemplateableValue::from_template(
                        "${backoff.interval}".to_string(),
                    ),
                },
                restart_exit_codes: vec![],
            },
        };
        let expected_output = Executable {
            path: TemplateableValue::new("/usr/bin/myapp".to_string())
                .with_template("${path}".to_string()),
            args: TemplateableValue::new(Args(
                "--config /etc/myapp.conf config_path integration_path".to_string(),
            ))
            .with_template("${args} ${config} ${integrations}".to_string()),
            env: TemplateableValue::new(Env("MYAPP_PORT=8080".to_string()))
                .with_template("MYAPP_PORT=${env.MYAPP_PORT}".to_string()),
            restart_policy: RestartPolicyConfig {
                backoff_strategy: BackoffStrategyConfig {
                    backoff_type: TemplateableValue::new(BackoffStrategyType::Linear)
                        .with_template("${backoff.type}".to_string()),
                    backoff_delay: TemplateableValue::new(BackoffDuration::from_secs(10))
                        .with_template("${backoff.delay}".to_string()),
                    max_retries: TemplateableValue::new(30)
                        .with_template("${backoff.retries}".to_string()),
                    last_retry_interval: TemplateableValue::new(BackoffDuration::from_secs(300))
                        .with_template("${backoff.interval}".to_string()),
                },
                restart_exit_codes: vec![],
            },
        };
        let actual_output = input.template_with(&variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }

    #[test]
    fn test_template_value_mapping() {
        let variables = NormalizedVariables::from([
            (
                "change.me.string".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::String("CHANGED-STRING".to_string())),
                    ..EndSpec::default_with_type(VariableType::String)
                },
            ),
            (
                "change.me.bool".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Bool(true)),
                    ..EndSpec::default_with_type(VariableType::Bool)
                },
            ),
            (
                "change.me.number".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Number(PosInt(42))),
                    ..EndSpec::default_with_type(VariableType::Number)
                },
            ),
        ]);
        let input: serde_yaml::Mapping = serde_yaml::from_str(
            r#"
        a_string: "${change.me.string}"
        a_boolean: "${change.me.bool}"
        a_number: "${change.me.number}"
        ${change.me.string}: "Do not scape me"
        ${change.me.bool}: "Do not scape me"
        ${change.me.number}: "Do not scape me"
        "#,
        )
        .unwrap();
        let expected_output: serde_yaml::Mapping = serde_yaml::from_str(
            r#"
        a_string: "CHANGED-STRING"
        a_boolean: true
        a_number: 42
        ${change.me.string}: "Do not scape me"
        ${change.me.bool}: "Do not scape me"
        ${change.me.number}: "Do not scape me"
        "#,
        )
        .unwrap();

        let actual_output = input.template_with(&variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }

    #[test]
    fn test_template_value_sequence() {
        let variables = NormalizedVariables::from([
            (
                "change.me.string".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::String("CHANGED-STRING".to_string())),
                    ..EndSpec::default_with_type(VariableType::String)
                },
            ),
            (
                "change.me.bool".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Bool(true)),
                    ..EndSpec::default_with_type(VariableType::Bool)
                },
            ),
            (
                "change.me.number".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Number(PosInt(42))),
                    ..EndSpec::default_with_type(VariableType::Number)
                },
            ),
        ]);
        let input: serde_yaml::Sequence = serde_yaml::from_str(
            r#"
        - ${change.me.string}
        - ${change.me.bool}
        - ${change.me.number}
        - Do not scape me
        "#,
        )
        .unwrap();
        let expected_output: serde_yaml::Sequence = serde_yaml::from_str(
            r#"
        - CHANGED-STRING
        - true
        - 42
        - Do not scape me
        "#,
        )
        .unwrap();

        let actual_output = input.template_with(&variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }

    #[test]
    fn test_template_yaml() {
        let variables = NormalizedVariables::from([
            (
                "change.me.string".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::String("CHANGED-STRING".to_string())),
                    ..EndSpec::default_with_type(VariableType::String)
                },
            ),
            (
                "change.me.bool".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Bool(true)),
                    ..EndSpec::default_with_type(VariableType::Bool)
                },
            ),
            (
                "change.me.number".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Number(PosInt(42))),
                    ..EndSpec::default_with_type(VariableType::Number)
                },
            ),
            (
                "change.me.yaml".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Yaml(
                        r#"{"key": "value"}"#.to_string().try_into().unwrap(),
                    )),
                    ..EndSpec::default_with_type(VariableType::Yaml)
                },
            ),
            (
                // Expansion inside variable's values is not supported.
                "yaml.with.var.placeholder".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Yaml(
                        r#"{"this.will.not.be.expanded": "${change.me.string}"}"#
                            .to_string()
                            .try_into()
                            .unwrap(),
                    )),
                    ..EndSpec::default_with_type(VariableType::Yaml)
                },
            ),
        ]);
        let input: serde_yaml::Value = serde_yaml::from_str(
            r#"
        an_object:
            a_string: ${change.me.string}
            a_boolean: ${change.me.bool}
            a_number: ${change.me.number}
        a_sequence:
            - ${change.me.string}
            - ${change.me.bool}
            - ${change.me.number}
        a_nested_object:
            with_nested_sequence:
                - a_string: ${change.me.string}
                - a_boolean: ${change.me.bool}
                - a_number: ${change.me.number}
                - a_yaml: ${change.me.yaml}
        a_string: ${change.me.string}
        a_boolean: ${change.me.bool}
        a_number: ${change.me.number}
        a_yaml: ${change.me.yaml}
        another_yaml: ${yaml.with.var.placeholder} # A variable inside another variable value is not expanded
        string_key: "here, the value ${change.me.yaml} is encoded as string because it is not alone"
        "#,
        )
        .unwrap();
        let expected_output: serde_yaml::Value = serde_yaml::from_str(
            r#"
        an_object:
            a_string: "CHANGED-STRING"
            a_boolean: true
            a_number: 42
        a_sequence:
            - "CHANGED-STRING"
            - true
            - 42
        a_nested_object:
            with_nested_sequence:
                - a_string: "CHANGED-STRING"
                - a_boolean: true
                - a_number: 42
                - a_yaml:
                    key:
                      value
        a_string: "CHANGED-STRING"
        a_boolean: true
        a_number: 42
        a_yaml:
          key: value
        another_yaml:
          "this.will.not.be.expanded": "${change.me.string}" # A variable inside another other variable value is not expanded
        string_key: "here, the value {\"key\": \"value\"} is encoded as string because it is not alone"
        "#,
        )
        .unwrap();

        let actual_output: serde_yaml::Value = input.template_with(&variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }

    #[test]
    fn test_template_yaml_value_string() {
        let variables = NormalizedVariables::from([
            (
                "simple.string.var".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::String("Value".into())),
                    ..EndSpec::default_with_type(VariableType::String)
                },
            ),
            (
                "string.with.yaml.var".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::String("[Value]".into())),
                    ..EndSpec::default_with_type(VariableType::String)
                },
            ),
            (
                "bool.var".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Bool(true)),
                    ..EndSpec::default_with_type(VariableType::Bool)
                },
            ),
            (
                "number.var".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Number(PosInt(42))),
                    ..EndSpec::default_with_type(VariableType::Number)
                },
            ),
            (
                "yaml.var".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Yaml(
                        r#"{"key": "value"}"#.to_string().try_into().unwrap(),
                    )),
                    ..EndSpec::default_with_type(VariableType::Yaml)
                },
            ),
        ]);

        assert_eq!(
            serde_yaml::Value::String("Value".into()),
            template_yaml_value_string("${simple.string.var}".into(), &variables).unwrap()
        );
        assert_eq!(
            serde_yaml::Value::String("var=Value".into()),
            template_yaml_value_string("var=${simple.string.var}".into(), &variables).unwrap()
        );
        assert_eq!(
            serde_yaml::Value::String("ValueValue".into()),
            template_yaml_value_string(
                "${simple.string.var}${simple.string.var}".into(),
                &variables
            )
            .unwrap()
        );
        assert_eq!(
            serde_yaml::Value::String("[Value]".into()),
            template_yaml_value_string("${string.with.yaml.var}".into(), &variables).unwrap()
        );
        // yaml, bool and number values are got when the corresponding variable is "alone".
        assert_eq!(
            serde_yaml::Value::Bool(true),
            template_yaml_value_string("${bool.var}".into(), &variables).unwrap()
        );
        assert_eq!(
            serde_yaml::Value::Number(serde_yaml::Number::try_from(42i32).unwrap()),
            template_yaml_value_string("${number.var}".into(), &variables).unwrap()
        );
        assert_eq!(
            serde_yaml::Value::String("truetrue".into()),
            template_yaml_value_string("${bool.var}${bool.var}".into(), &variables).unwrap()
        );
        assert_eq!(
            serde_yaml::Value::String("true42".into()),
            template_yaml_value_string("${bool.var}${number.var}".into(), &variables).unwrap()
        );
        assert_eq!(
            serde_yaml::Value::String("the 42 Value is true".into()),
            template_yaml_value_string(
                "the ${number.var} ${simple.string.var} is ${bool.var}".into(),
                &variables
            )
            .unwrap()
        );
        let m = assert_matches!(
            template_yaml_value_string("${yaml.var}".into(), &variables).unwrap(),
            serde_yaml::Value::Mapping(m) => m
        );
        assert_eq!(
            serde_yaml::Value::String("value".into()),
            m.get("key").unwrap().clone()
        );
        assert_eq!(
            serde_yaml::Value::String(r#"x: {"key": "value"}"#.into()),
            template_yaml_value_string("x: ${yaml.var}".into(), &variables).unwrap()
        )
    }

    #[test]
    fn test_normalized_var() {
        let variables = NormalizedVariables::from([(
            "var.name".to_string(),
            EndSpec::default_with_type(VariableType::String),
        )]);

        assert_eq!(
            normalized_var("var.name", &variables).unwrap().type_,
            VariableType::String
        );
        let key = assert_matches!(
            normalized_var("does.not.exists", &variables).err().unwrap(),
            AgentTypeError::MissingTemplateKey(s) => s);
        assert_eq!("does.not.exists".to_string(), key);
    }

    #[test]
    fn test_replace() {
        let value_var = EndSpec {
            final_value: Some(TrivialValue::String("Value".into())),
            ..EndSpec::default_with_type(VariableType::String)
        };
        let default_var = EndSpec {
            default: Some(TrivialValue::String("Default".into())),
            ..EndSpec::default_with_type(VariableType::String)
        };
        let neither_value_nor_default = EndSpec::default_with_type(VariableType::String);

        let re = template_re();
        assert_eq!(
            "Value-${other}".to_string(),
            replace(re, "${any}-${other}", "any", &value_var).unwrap()
        );
        assert_eq!(
            "Default-${other}".to_string(),
            replace(re, "${any}-${other}", "any", &default_var).unwrap()
        );
        let key = assert_matches!(
            replace(re, "${any}-x", "any", &neither_value_nor_default).err().unwrap(),
            AgentTypeError::MissingTemplateKey(s) => s);
        assert_eq!("any".to_string(), key);
    }
}
