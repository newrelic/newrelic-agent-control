use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use tracing::warn;

use super::{
    agent_types::NormalizedVariables,
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

    let result = re
        .find_iter(&s)
        .map(|i| i.as_str())
        .try_fold(s.clone(), |r, i| {
            let trimmed_s = i
                .trim_start_matches(TEMPLATE_BEGIN)
                .trim_end_matches(TEMPLATE_END);
            if !variables.contains_key(trimmed_s) {
                return Err(AgentTypeError::MissingTemplateKey(trimmed_s.to_string()));
            }
            let replacement = variables[trimmed_s].clone();
            Ok(re
                .replace(
                    &r,
                    replacement
                        .final_value
                        .or(replacement.default)
                        .ok_or(AgentTypeError::MissingTemplateKey(trimmed_s.to_string()))?
                        .to_string(),
                )
                .to_string())
        });
    result
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
            serde_yaml::Value::String(st) => template_value_string(st, variables)?,
            _ => self,
        };

        Ok(templated_value)
    }
}

impl Templateable for serde_yaml::Mapping {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        for (_, mut v) in self.iter() {
            v = &v.clone().template_with(variables)?;
        }
        Ok(self)
    }
}

impl Templateable for serde_yaml::Sequence {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        for mut v in self.iter() {
            v = &v.clone().template_with(variables)?;
        }
        Ok(self)
    }
}

fn template_value_string(
    st: String,
    variables: &NormalizedVariables,
) -> Result<serde_yaml::Value, AgentTypeError> {
    /*
    // TODO: All templated values are a YAML String, but the result does not have to be a string.
    ```yaml
    test: ${value}
    ```
    Given the YAML above, if `value` is `1` the result is `"1"`, not a `1` (integer).
    We will have to be careful to cast because there are many reason se do not want to change the type from string
    to int, like in Kubernetes annotations that are a map[string]string and refuses to cast booleans of integers to
    string.

    In a future we might want to do a spike to support Tagged values: https://docs.rs/serde_yaml/latest/serde_yaml/enum.Value.html#variant.Tagged

    For now, as a first iteration, we simply return a string and template a string.
    */
    let templated = template_string(st, variables)?;

    Ok(serde_yaml::Value::String(templated))
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
    use crate::config::agent_type::restart_policy::{BackoffDuration, BackoffStrategyType};
    use crate::config::agent_type::trivial_value::N::PosInt;
    use crate::config::agent_type::{
        agent_types::{EndSpec, TemplateableValue, VariableType},
        runtime_config::{Args, Env},
        trivial_value::{TrivialValue, N},
    };

    use super::*;

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
        ]);

        let input = Executable {
            path: TemplateableValue::from_template("${path}".to_string()),
            args: TemplateableValue::from_template("${args}".to_string()),
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
            args: TemplateableValue::new(Args("--config /etc/myapp.conf".to_string()))
                .with_template("${args}".to_string()),
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
    fn test_template_yaml() {
        let variables = NormalizedVariables::from([
            (
                "change.me.string".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::String("CHANGED-STRING".to_string())),
                    default: None,
                    description: String::default(),
                    required: true,
                    type_: VariableType::String,
                    file_path: None,
                },
            ),
            (
                "change.me.bool".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Bool(true)),
                    default: None,
                    description: String::default(),
                    required: true,
                    type_: VariableType::Bool,
                    file_path: None,
                },
            ),
            (
                "change.me.number".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Number(PosInt(42))),
                    default: None,
                    description: String::default(),
                    required: true,
                    type_: VariableType::Number,
                    file_path: None,
                },
            ),
        ]);
        let input: serde_yaml::Value = serde_yaml::from_str(
            r#"
        a_string: ${change.me.string}
        a_boolean: ${change.me.bool}
        a_number: ${change.me.number}
        ${change.me.string}: Do not scape me
        ${change.me.bool}: Do not scape me
        ${change.me.number}: Do not scape me
        "#,
        )
        .unwrap();
        let expected_output: serde_yaml::Value = serde_yaml::from_str(
            r#"
        a_string: "CHANGED-STRING"
        a_boolean: "true"  # TODO: This test should break in a future iteration.
        a_number: "42"  # TODO: This test should break in a future iteration.
        ${change.me.string}: Do not scape me
        ${change.me.bool}: Do not scape me
        ${change.me.number}: Do not scape me
        "#,
        )
        .unwrap();

        let actual_output: serde_yaml::Value = input.template_with(&variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }

    #[test]
    fn test_template_nested_yaml() {
        let variables = NormalizedVariables::from([
            (
                "change.me.string".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::String("CHANGED-STRING".to_string())),
                    default: None,
                    description: String::default(),
                    required: true,
                    type_: VariableType::String,
                    file_path: None,
                },
            ),
            (
                "change.me.bool".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Bool(true)),
                    default: None,
                    description: String::default(),
                    required: true,
                    type_: VariableType::Bool,
                    file_path: None,
                },
            ),
            (
                "change.me.number".to_string(),
                EndSpec {
                    final_value: Some(TrivialValue::Number(PosInt(42))),
                    default: None,
                    description: String::default(),
                    required: true,
                    type_: VariableType::Number,
                    file_path: None,
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
        a_string: ${change.me.string}
        a_boolean: ${change.me.bool}
        a_number: ${change.me.number}
        "#,
        )
        .unwrap();
        let expected_output: serde_yaml::Value = serde_yaml::from_str(
            r#"
        an_object:
            a_string: "CHANGED-STRING"
            a_boolean: "true"  # TODO: This test should break in a future iteration.
            a_number: "42"  # TODO: This test should break in a future iteration.
        a_sequence:
            - "CHANGED-STRING"
            - "true"  # TODO: This test should break in a future iteration.
            - "42"  # TODO: This test should break in a future iteration.
        a_nested_object:
            with_nested_sequence:
                - a_string: "CHANGED-STRING"
                - a_boolean: "true"  # TODO: This test should break in a future iteration.
                - a_number: "42"  # TODO: This test should break in a future iteration.
        a_string: "CHANGED-STRING"
        a_boolean: "true"  # TODO: This test should break in a future iteration.
        a_number: "42"  # TODO: This test should break in a future iteration.
        "#,
        )
        .unwrap();

        let actual_output: serde_yaml::Value = input.template_with(&variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }
}
