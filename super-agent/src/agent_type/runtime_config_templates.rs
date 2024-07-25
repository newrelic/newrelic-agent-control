use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use regex::Regex;
use tracing::warn;

use super::definition::Variables;
use super::variable::definition::VariableDefinition;
use super::variable::kind::Kind;
use super::{
    error::AgentTypeError,
    restart_policy::{BackoffStrategyConfig, RestartPolicyConfig},
    runtime_config::{Deployment, Executable, K8s, K8sObject, K8sObjectMeta, OnHost, Runtime},
};

/// Regex that extracts the template values from a string.
///
/// Example:
///
/// ```
/// use regex::Regex;
///
/// const TEMPLATE_RE: &str = r"\$\{(nr-[a-z]+:[a-zA-Z0-9\.\-_/]+)\}";
/// let re = Regex::new(TEMPLATE_RE).unwrap();
/// let content = "Hello ${nr-var:name.value}!";
///
/// let result = re.find_iter(content).map(|i| i.as_str()).collect::<Vec<_>>();
///
/// assert_eq!(result, vec!["${nr-var:name.value}"]);
const TEMPLATE_RE: &str = r"\$\{(nr-[a-z]+:[a-zA-Z0-9\.\-_/]+)\}";
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
    variables: &'a Variables,
) -> Result<&'a VariableDefinition, AgentTypeError> {
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
    normalized_var: &VariableDefinition,
) -> Result<String, AgentTypeError> {
    let value = normalized_var
        .get_template_value()
        .ok_or(AgentTypeError::MissingTemplateKey(var_name.to_string()))?
        .to_string();

    Ok(re.replace(s, value).to_string())
}

pub trait Templateable {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError>
    where
        Self: std::marker::Sized;
}

impl Templateable for Executable {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            path: self.path.template_with(variables)?,
            args: self.args.template_with(variables)?,
            env: self.env.template_with(variables)?,
            restart_policy: self.restart_policy.template_with(variables)?,
            health: self
                .health
                .map(|health| health.template_with(variables))
                .transpose()?,
        })
    }
}

// The actual std type that has a meaningful implementation of Templateable
impl Templateable for String {
    fn template_with(self, variables: &Variables) -> Result<String, AgentTypeError> {
        template_string(self, variables)
    }
}

fn template_string(s: String, variables: &Variables) -> Result<String, AgentTypeError> {
    let re = template_re();
    re.find_iter(&s)
        .map(|i| i.as_str())
        .try_fold(s.clone(), |r, i| {
            let var_name = template_trim(i);
            replace(re, &r, var_name, normalized_var(var_name, variables)?)
        })
}

impl Templateable for OnHost {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            executables: self
                .executables
                .into_iter()
                .map(|e| e.template_with(variables))
                .collect::<Result<Vec<Executable>, AgentTypeError>>()?,
            enable_file_logging: self.enable_file_logging.template_with(variables)?,
        })
    }
}

impl Templateable for RestartPolicyConfig {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            backoff_strategy: self.backoff_strategy.template_with(variables)?,
            restart_exit_codes: self.restart_exit_codes, // TODO Not templating this for now!
        })
    }
}

impl Templateable for BackoffStrategyConfig {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
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
            warn!("Backoff strategy type is set to `none`, but some of the backoff strategy fields are set. They will be ignored");
        }

        Ok(result)
    }
}

impl Templateable for K8s {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            objects: self
                .objects
                .into_iter()
                .map(|(k, v)| Ok((k, v.template_with(variables)?)))
                .collect::<Result<HashMap<String, K8sObject>, AgentTypeError>>()?,
            health: self.health,
        })
    }
}

impl Templateable for K8sObject {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            api_version: self.api_version.clone(),
            kind: self.kind.clone(),
            metadata: self.metadata.template_with(variables)?,
            fields: self.fields.template_with(variables)?,
        })
    }
}

impl Templateable for K8sObjectMeta {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            labels: self
                .labels
                .into_iter()
                .map(|(k, v)| Ok((k.template_with(variables)?, v.template_with(variables)?)))
                .collect::<Result<BTreeMap<String, String>, AgentTypeError>>()?,
            name: self.name.template_with(variables)?,
        })
    }
}

impl Templateable for serde_yaml::Value {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
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
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        self.into_iter()
            .map(|(k, v)| Ok((k, v.template_with(variables)?)))
            .collect()
    }
}

impl Templateable for serde_yaml::Sequence {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
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
    variables: &Variables,
) -> Result<serde_yaml::Value, AgentTypeError> {
    // When there is more content than a variable template, template as a regular string.
    if !only_template_var_re().is_match(s.as_str()) {
        let templated = template_string(s, variables)?;
        return Ok(serde_yaml::Value::String(templated));
    }
    // Otherwise, template according to the variable type.
    let var_name = template_trim(s.as_str());
    let var_spec = normalized_var(var_name, variables)?;
    let var_value = var_spec
        .get_template_value()
        .ok_or(AgentTypeError::MissingRequiredKey(var_name.to_string()))?;
    match var_spec.kind() {
        Kind::Yaml(y) => Ok(y
            .get_final_value()
            .cloned()
            .expect("a final value must be present at this point")),

        Kind::Bool(_) | Kind::Number(_) => {
            serde_yaml::from_str(var_value.to_string().as_str()).map_err(AgentTypeError::SerdeYaml)
        }
        _ => Ok(serde_yaml::Value::String(var_value.to_string())),
    }
}

impl Templateable for Deployment {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
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

impl Templateable for Runtime {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            deployment: self.deployment.template_with(variables)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;
    use serde_yaml::Number;

    use std::collections::HashMap;

    use crate::agent_type::restart_policy::{BackoffDelay, BackoffLastRetryInterval};
    use crate::agent_type::{
        definition::TemplateableValue,
        restart_policy::BackoffStrategyType,
        runtime_config::{Args, Env},
        trivial_value::FilePathWithContent,
    };

    use super::*;

    #[test]
    fn test_template_string() {
        let variables = Variables::from([
            (
                "nr-var:name".to_string(),
                VariableDefinition::new(String::default(), true, None, Some("Alice".to_string())),
            ),
            (
                "nr-var:age".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(Number::from(30))),
            ),
        ]);

        let input = "Hello ${nr-var:name}! You are ${nr-var:age} years old.".to_string();
        let expected_output = "Hello Alice! You are 30 years old.".to_string();
        let actual_output = template_string(input, &variables).unwrap();
        assert_eq!(actual_output, expected_output);
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
            env: TemplateableValue::from_template(
                "MYAPP_PORT=${nr-var:env.MYAPP_PORT}".to_string(),
            ),
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
            health: None,
        };
        let expected_output = Executable {
            path: TemplateableValue::new("/usr/bin/myapp".to_string())
                .with_template("${nr-var:path}".to_string()),
            args: TemplateableValue::new(Args(
                "--config /etc/myapp.conf config_path integration_path".to_string(),
            ))
            .with_template("${nr-var:args} ${nr-var:config} ${nr-var:integrations}".to_string()),
            env: TemplateableValue::new(Env("MYAPP_PORT=8080".to_string()))
                .with_template("MYAPP_PORT=${nr-var:env.MYAPP_PORT}".to_string()),
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
            health: None,
        };
        let actual_output = input.template_with(&variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }

    #[test]
    fn test_template_value_mapping() {
        let variables = Variables::from([
            (
                "nr-var:change.me.string".to_string(),
                VariableDefinition::new(
                    String::default(),
                    true,
                    None,
                    Some("CHANGED-STRING".to_string()),
                ),
            ),
            (
                "nr-var:change.me.bool".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(true)),
            ),
            (
                "nr-var:change.me.number".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(Number::from(42))),
            ),
        ]);
        let input: serde_yaml::Mapping = serde_yaml::from_str(
            r#"
        a_string: "${nr-var:change.me.string}"
        a_boolean: "${nr-var:change.me.bool}"
        a_number: "${nr-var:change.me.number}"
        ${nr-var:change.me.string}: "Do not scape me"
        ${nr-var:change.me.bool}: "Do not scape me"
        ${nr-var:change.me.number}: "Do not scape me"
        "#,
        )
        .unwrap();
        let expected_output: serde_yaml::Mapping = serde_yaml::from_str(
            r#"
        a_string: "CHANGED-STRING"
        a_boolean: true
        a_number: 42
        ${nr-var:change.me.string}: "Do not scape me"
        ${nr-var:change.me.bool}: "Do not scape me"
        ${nr-var:change.me.number}: "Do not scape me"
        "#,
        )
        .unwrap();

        let actual_output = input.template_with(&variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }

    #[test]
    fn test_template_value_sequence() {
        let variables = Variables::from([
            (
                "nr-var:change.me.string".to_string(),
                VariableDefinition::new(
                    String::default(),
                    true,
                    None,
                    Some("CHANGED-STRING".to_string()),
                ),
            ),
            (
                "nr-var:change.me.bool".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(true)),
            ),
            (
                "nr-var:change.me.number".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(Number::from(42))),
            ),
        ]);
        let input: serde_yaml::Sequence = serde_yaml::from_str(
            r#"
        - ${nr-var:change.me.string}
        - ${nr-var:change.me.bool}
        - ${nr-var:change.me.number}
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
        let variables = Variables::from([
            (
                "nr-var:change.me.string".to_string(),
                VariableDefinition::new(
                    String::default(),
                    true,
                    None,
                    Some("CHANGED-STRING".to_string()),
                ),
            ),
            (
                "nr-var:change.me.bool".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(true)),
            ),
            (
                "nr-var:change.me.number".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(Number::from(42))),
            ),
            (
                "nr-var:change.me.yaml".to_string(),
                VariableDefinition::new(
                    String::default(),
                    true,
                    None,
                    Some(serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter(
                        [("key".into(), "value".into())],
                    ))),
                ),
            ),
            (
                // Expansion inside variable's values is not supported.
                "nr-var:yaml.with.var.placeholder".to_string(),
                VariableDefinition::new(
                    String::default(),
                    true,
                    None,
                    Some(serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter(
                        [(
                            "this.will.not.be.expanded".into(),
                            "${nr-var:change.me.string}".into(),
                        )],
                    ))),
                ),
            ),
        ]);
        let input: serde_yaml::Value = serde_yaml::from_str(
            r#"
        an_object:
            a_string: ${nr-var:change.me.string}
            a_boolean: ${nr-var:change.me.bool}
            a_number: ${nr-var:change.me.number}
        a_sequence:
            - ${nr-var:change.me.string}
            - ${nr-var:change.me.bool}
            - ${nr-var:change.me.number}
        a_nested_object:
            with_nested_sequence:
                - a_string: ${nr-var:change.me.string}
                - a_boolean: ${nr-var:change.me.bool}
                - a_number: ${nr-var:change.me.number}
                - a_yaml: ${nr-var:change.me.yaml}
        a_string: ${nr-var:change.me.string}
        a_boolean: ${nr-var:change.me.bool}
        a_number: ${nr-var:change.me.number}
        a_yaml: ${nr-var:change.me.yaml}
        another_yaml: ${nr-var:yaml.with.var.placeholder} # A variable inside another variable value is not expanded
        string_key: "here, the value ${nr-var:change.me.yaml} is encoded as string because it is not alone"
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
          "this.will.not.be.expanded": "${nr-var:change.me.string}" # A variable inside another other variable value is not expanded
        string_key: "here, the value key: value\n is encoded as string because it is not alone"
        "#, // FIXME? Note line above, the "key: value\n" part was replaced!!
        )
        .unwrap();

        let actual_output: serde_yaml::Value = input.template_with(&variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }

    #[test]
    fn test_template_yaml_value_string() {
        let variables = Variables::from([
            (
                "nr-var:simple.string.var".to_string(),
                VariableDefinition::new(String::default(), true, None, Some("Value".to_string())),
            ),
            (
                "nr-var:string.with.yaml.var".to_string(),
                VariableDefinition::new(String::default(), true, None, Some("[Value]".to_string())),
            ),
            (
                "nr-var:bool.var".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(true)),
            ),
            (
                "nr-var:number.var".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(Number::from(42))),
            ),
            (
                "nr-var:yaml.var".to_string(),
                VariableDefinition::new(
                    String::default(),
                    true,
                    None,
                    Some(serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter(
                        [("key".into(), "value".into())],
                    ))),
                ),
            ),
        ]);

        assert_eq!(
            serde_yaml::Value::String("Value".into()),
            template_yaml_value_string("${nr-var:simple.string.var}".into(), &variables).unwrap()
        );
        assert_eq!(
            serde_yaml::Value::String("var=Value".into()),
            template_yaml_value_string("var=${nr-var:simple.string.var}".into(), &variables)
                .unwrap()
        );
        assert_eq!(
            serde_yaml::Value::String("ValueValue".into()),
            template_yaml_value_string(
                "${nr-var:simple.string.var}${nr-var:simple.string.var}".into(),
                &variables
            )
            .unwrap()
        );
        assert_eq!(
            serde_yaml::Value::String("[Value]".into()),
            template_yaml_value_string("${nr-var:string.with.yaml.var}".into(), &variables)
                .unwrap()
        );
        // yaml, bool and number values are got when the corresponding variable is "alone".
        assert_eq!(
            serde_yaml::Value::Bool(true),
            template_yaml_value_string("${nr-var:bool.var}".into(), &variables).unwrap()
        );
        assert_eq!(
            serde_yaml::Value::Number(serde_yaml::Number::from(42i32)),
            template_yaml_value_string("${nr-var:number.var}".into(), &variables).unwrap()
        );
        assert_eq!(
            serde_yaml::Value::String("truetrue".into()),
            template_yaml_value_string("${nr-var:bool.var}${nr-var:bool.var}".into(), &variables)
                .unwrap()
        );
        assert_eq!(
            serde_yaml::Value::String("true42".into()),
            template_yaml_value_string("${nr-var:bool.var}${nr-var:number.var}".into(), &variables)
                .unwrap()
        );
        assert_eq!(
            serde_yaml::Value::String("the 42 Value is true".into()),
            template_yaml_value_string(
                "the ${nr-var:number.var} ${nr-var:simple.string.var} is ${nr-var:bool.var}".into(),
                &variables
            )
            .unwrap()
        );
        let m = assert_matches!(
            template_yaml_value_string("${nr-var:yaml.var}".into(), &variables).unwrap(),
            serde_yaml::Value::Mapping(m) => m
        );
        assert_eq!(
            serde_yaml::Value::String("value".into()),
            m.get("key").unwrap().clone()
        );
        assert_eq!(
            serde_yaml::Value::String("x: key: value\n".into()), // FIXME? Consder if this is ok.
            template_yaml_value_string("x: ${nr-var:yaml.var}".into(), &variables).unwrap()
        )
    }

    #[test]
    fn test_normalized_var() {
        let variables = Variables::from([(
            "nr-var:var.name".to_string(),
            VariableDefinition::new(String::default(), true, None, Some("Value".to_string())),
        )]);

        assert_matches!(
            normalized_var("nr-var:var.name", &variables)
                .unwrap()
                .kind(),
            Kind::String(_)
        );
        let key = assert_matches!(
            normalized_var("does.not.exists", &variables).unwrap_err(),
            AgentTypeError::MissingTemplateKey(s) => s);
        assert_eq!("does.not.exists".to_string(), key);
    }

    #[test]
    fn test_replace() {
        let value_var =
            VariableDefinition::new(String::default(), true, None, Some("Value".to_string()));
        let default_var =
            VariableDefinition::new(String::default(), true, Some("Default".to_string()), None);

        let neither_value_nor_default =
            VariableDefinition::new(String::default(), true, None::<String>, None::<String>);

        let re = template_re();
        assert_eq!(
            "Value-${nr-var:other}".to_string(),
            replace(re, "${nr-var:any}-${nr-var:other}", "any", &value_var).unwrap()
        );
        assert_eq!(
            "Default-${nr-var:other}".to_string(),
            replace(re, "${nr-var:any}-${nr-var:other}", "any", &default_var).unwrap()
        );
        let key = assert_matches!(
            replace(re, "${nr-var:any}-x", "any", &neither_value_nor_default).unwrap_err(),
            AgentTypeError::MissingTemplateKey(s) => s);
        assert_eq!("any".to_string(), key);
    }

    #[test]
    fn test_template_k8s() {
        let untouched_val = "${nr-var:any} no templated";
        let test_agent_id = "id";
        let k8s_template: K8s = serde_yaml::from_str(
            format!(
                r#"
objects:
  cr1:
    apiVersion: {untouched_val}
    kind: {untouched_val}
    metadata:
      name: ${{nr-sub:agent_id}}
      labels:
        foo: ${{nr-var:any}}
        ${{nr-var:any}}: bar
    spec: ${{nr-var:any}}
"#
            )
            .as_str(),
        )
        .unwrap();

        let value = "test_value";
        let variables = Variables::from([
            (
                "nr-var:any".to_string(),
                VariableDefinition::new(String::default(), true, None, Some(value.to_string())),
            ),
            (
                "nr-sub:agent_id".to_string(),
                VariableDefinition::new_final_string_variable(test_agent_id.to_string()),
            ),
        ]);

        let k8s = k8s_template.template_with(&variables).unwrap();

        let cr1 = k8s.objects.get("cr1").unwrap().clone();

        // Expect no template applied on these fields.
        assert_eq!(cr1.api_version, untouched_val);
        assert_eq!(cr1.kind, untouched_val);

        // Expect template works on fields.
        assert_eq!(cr1.fields.get("spec").unwrap(), value);

        // Expect template works on name.
        assert_eq!(cr1.metadata.name, test_agent_id);

        // Expect template works on labels.
        let labels = cr1.metadata.labels;
        assert_eq!(labels.get("foo").unwrap(), value);
        assert_eq!(labels.get(value).unwrap(), "bar");
    }
}
