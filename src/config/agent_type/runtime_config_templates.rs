use regex::Regex;
use tracing::warn;

use super::{
    agent_types::NormalizedVariables,
    error::AgentTypeError,
    restart_policy::{BackoffStrategyConfig, RestartPolicyConfig},
    runtime_config::{Deployment, Executable, OnHost, RuntimeConfig},
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
    let re = Regex::new(TEMPLATE_RE).unwrap();

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
                        .kind
                        .get_final_value()
                        .or(replacement.kind.get_default())
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
        Ok(Self { on_host: oh })
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
    use crate::config::agent_type::agent_types::{Kind, KindValue};
    use crate::config::agent_type::restart_policy::{BackoffDuration, BackoffStrategyType};
    use crate::config::agent_type::{
        agent_types::{EndSpec, TemplateableValue},
        runtime_config::{Args, Env},
        trivial_value::Number,
    };

    use super::*;

    #[test]
    fn test_template_string() {
        let name_endspec: EndSpec = EndSpec {
            description: String::default(),
            kind: Kind::String(KindValue {
                final_value: Some("Alice".to_string()),
                required: true,
                default: None,
                variants: None,
                file_path: Some("some_path".to_string()),
            }),
        };
        let age_endspec: EndSpec = EndSpec {
            description: String::default(),
            kind: Kind::Number(KindValue {
                final_value: Some(Number::PosInt(30)),
                required: true,
                default: None,
                variants: None,
                file_path: Some("some_path".to_string()),
            }),
        };

        let variables = NormalizedVariables::from([
            ("name".to_string(), name_endspec),
            ("age".to_string(), age_endspec),
        ]);

        let input = "Hello ${name}! You are ${age} years old.".to_string();
        let expected_output = "Hello Alice! You are 30 years old.".to_string();
        let actual_output = template_string(input, &variables).unwrap();
        assert_eq!(actual_output, expected_output);
    }

    #[test]
    fn test_template_executable() {
        let path_endspec: EndSpec = EndSpec {
            description: String::default(),
            kind: Kind::String(KindValue {
                final_value: Some("/usr/bin/myapp".to_string()),
                required: true,
                default: None,
                variants: None,
                file_path: Some("some_path".to_string()),
            }),
        };
        let args_endspec: EndSpec = EndSpec {
            description: String::default(),
            kind: Kind::String(KindValue {
                final_value: Some("--config /etc/myapp.conf".to_string()),
                required: true,
                default: None,
                variants: None,
                file_path: Some("some_path".to_string()),
            }),
        };
        let env_endspec: EndSpec = EndSpec {
            description: String::default(),
            kind: Kind::Number(KindValue {
                final_value: Some(Number::PosInt(8080)),
                required: true,
                default: None,
                variants: None,
                file_path: Some("some_path".to_string()),
            }),
        };
        let backofftype_endspec: EndSpec = EndSpec {
            description: "backoff_type".to_string(),
            kind: Kind::String(KindValue {
                final_value: Some("exponential".to_string()),
                required: true,
                default: None,
                variants: None, // FIXME???
                file_path: Some("some_path".to_string()),
            }),
        };
        let backoffdelay_endspec: EndSpec = EndSpec {
            description: "backoff_delay".to_string(),
            kind: Kind::String(KindValue {
                final_value: Some("10s".to_string()),
                required: true,
                default: None,
                variants: None,
                file_path: Some("some_path".to_string()),
            }),
        };
        let backoffretries_endspec: EndSpec = EndSpec {
            description: "backoff_retries".to_string(),
            kind: Kind::Number(KindValue {
                final_value: Some(Number::PosInt(30)),
                required: true,
                default: None,
                variants: None,
                file_path: Some("some_path".to_string()),
            }),
        };
        let backoffinterval_endspec: EndSpec = EndSpec {
            description: "backoff_interval".to_string(),
            kind: Kind::String(KindValue {
                final_value: Some("300s".to_string()),
                required: true,
                default: None,
                variants: None,
                file_path: Some("some_path".to_string()),
            }),
        };

        let variables = NormalizedVariables::from([
            ("path".to_string(), path_endspec),
            ("args".to_string(), args_endspec),
            ("env.MYAPP_PORT".to_string(), env_endspec),
            ("backoff.type".to_string(), backofftype_endspec),
            ("backoff.delay".to_string(), backoffdelay_endspec),
            ("backoff.retries".to_string(), backoffretries_endspec),
            ("backoff.interval".to_string(), backoffinterval_endspec),
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
                    backoff_type: TemplateableValue::new(BackoffStrategyType::Exponential)
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
}
