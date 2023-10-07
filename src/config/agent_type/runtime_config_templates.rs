use regex::Regex;

use super::{
    agent_types::NormalizedVariables,
    error::AgentTypeError,
    restart_policy::{BackoffStrategyConfig, BackoffStrategyInner, RestartPolicyConfig},
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
        })
    }
}

// The actual std type that has a meaningful implementation of Templateable
impl Templateable for String {
    fn template_with(self, variables: &NormalizedVariables) -> Result<String, AgentTypeError> {
        let re = Regex::new(TEMPLATE_RE).unwrap();

        let result = re
            .find_iter(&self.clone())
            .map(|i| i.as_str())
            .try_fold(self, |r, i| {
                let trimmed_s = i
                    .trim_start_matches(TEMPLATE_BEGIN)
                    .trim_end_matches(TEMPLATE_END);
                if !variables.contains_key(trimmed_s) {
                    return Err(AgentTypeError::MissingTemplateKey(trimmed_s.to_string()));
                }
                let replacement = &variables[trimmed_s];
                Ok(re
                    .replace(
                        &r,
                        replacement
                            .final_value
                            .as_ref()
                            .or(replacement.default.as_ref())
                            .ok_or(AgentTypeError::MissingTemplateKey(trimmed_s.to_string()))?
                            .to_string(),
                    )
                    .to_string())
            });
        result
    }
}

impl Templateable for OnHost {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            executables: self
                .executables
                .into_iter()
                .map(|e| e.template_with(variables))
                .collect::<Result<Vec<Executable>, AgentTypeError>>()?,
            restart_policy: self.restart_policy.template_with(variables)?,
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
        Ok(match self {
            BackoffStrategyConfig::None => BackoffStrategyConfig::None,
            BackoffStrategyConfig::Fixed(inner) => {
                BackoffStrategyConfig::Fixed(inner.template_with(variables)?)
            }
            BackoffStrategyConfig::Linear(inner) => {
                BackoffStrategyConfig::Linear(inner.template_with(variables)?)
            }
            BackoffStrategyConfig::Exponential(inner) => {
                BackoffStrategyConfig::Exponential(inner.template_with(variables)?)
            }
        })
    }
}

impl Templateable for BackoffStrategyInner {
    fn template_with(self, variables: &NormalizedVariables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            backoff_delay_seconds: self.backoff_delay_seconds.template_with(variables)?,
            max_retries: self.max_retries.template_with(variables)?,
            last_retry_interval_seconds: self
                .last_retry_interval_seconds
                .template_with(variables)?,
        })
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
