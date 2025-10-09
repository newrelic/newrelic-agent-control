use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::templates::Templateable;
use duration_str::deserialize_duration;
use serde::Deserialize;
use std::{str::FromStr, time::Duration};
use wrapper_with_default::WrapperWithDefault;

use super::templateable_value::TemplateableValue;

/// Defines the Restart Policy configuration.
/// This policy outlines the procedures followed for restarting agents when their execution encounters failure.
#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct RestartPolicyConfig {
    /// Strategy configuration to retry in case of failure.
    #[serde(default)]
    pub backoff_strategy: BackoffStrategyConfig,
    /// List of exit codes that triggers a restart.
    #[serde(default)]
    pub restart_exit_codes: Vec<i32>,
}

impl Templateable for RestartPolicyConfig {
    type Output = Self;

    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            backoff_strategy: self.backoff_strategy.template_with(variables)?,
            restart_exit_codes: self.restart_exit_codes, // TODO Not templating this for now!
        })
    }
}

/*
Default values for supervisor restarts
TODO: refine values with real executions
*/
pub(super) const DEFAULT_BACKOFF_DELAY: Duration = Duration::from_secs(2);
pub(super) const DEFAULT_BACKOFF_MAX_RETRIES: usize = 0;
pub(super) const DEFAULT_BACKOFF_LAST_RETRY_INTERVAL: Duration = Duration::from_secs(600);

#[derive(Debug, Deserialize, PartialEq, Clone, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_BACKOFF_DELAY)]
pub struct BackoffDelay(#[serde(deserialize_with = "deserialize_duration")] Duration);

impl BackoffDelay {
    pub fn from_secs(value: u64) -> Self {
        Self(Duration::from_secs(value))
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_BACKOFF_LAST_RETRY_INTERVAL)]
pub struct BackoffLastRetryInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);

impl BackoffLastRetryInterval {
    pub fn from_secs(value: u64) -> Self {
        Self(Duration::from_secs(value))
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_BACKOFF_MAX_RETRIES)]
pub struct MaxRetries(usize);

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(default)]
pub struct BackoffStrategyConfig {
    #[serde(rename = "type")]
    pub backoff_type: TemplateableValue<BackoffStrategyType>,
    pub backoff_delay: TemplateableValue<BackoffDelay>,
    pub max_retries: TemplateableValue<MaxRetries>,
    pub last_retry_interval: TemplateableValue<BackoffLastRetryInterval>,
}

impl Templateable for BackoffStrategyConfig {
    type Output = Self;

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
        Ok(result)
    }
}

#[derive(Debug, Deserialize, Default, PartialEq, Clone)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum BackoffStrategyType {
    #[default]
    Fixed,
    Linear,
    Exponential,
}

impl FromStr for BackoffStrategyType {
    type Err = AgentTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "fixed" => Ok(Self::Fixed),
            "linear" => Ok(Self::Linear),
            "exponential" => Ok(Self::Exponential),
            _ => Err(AgentTypeError::UnknownBackoffStrategyType(s.to_string())),
        }
    }
}

impl Default for BackoffStrategyConfig {
    fn default() -> Self {
        Self {
            backoff_type: TemplateableValue::new(BackoffStrategyType::Linear),
            backoff_delay: TemplateableValue::new(DEFAULT_BACKOFF_DELAY.into()),
            max_retries: TemplateableValue::new(DEFAULT_BACKOFF_MAX_RETRIES.into()),
            last_retry_interval: TemplateableValue::new(DEFAULT_BACKOFF_LAST_RETRY_INTERVAL.into()),
        }
    }
}
