//! Restart policy and backoff strategy configuration for on-host executables.
use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::templates::Templateable;
use duration_str::deserialize_duration;
use serde::Deserialize;
use std::{str::FromStr, time::Duration};
use wrapper_with_default::WrapperWithDefault;

use super::templateable_value::TemplateableValue;

pub mod rendered;

/// Defines the Restart Policy configuration.
/// This policy outlines the procedures followed for restarting agents when their execution encounters failure.
#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct RestartPolicyConfig {
    /// Strategy configuration to retry in case of failure.
    #[serde(default)]
    pub backoff_strategy: BackoffStrategyConfig,
}

impl Templateable for RestartPolicyConfig {
    type Output = rendered::RestartPolicyConfig;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        Ok(Self::Output {
            backoff_strategy: self.backoff_strategy.template_with(variables)?,
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

/// The delay applied before retrying a failed execution.
#[derive(Debug, Deserialize, PartialEq, Clone, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_BACKOFF_DELAY)]
pub struct BackoffDelay(#[serde(deserialize_with = "deserialize_duration")] Duration);

impl BackoffDelay {
    /// Builds a delay of the given number of seconds.
    pub fn from_secs(value: u64) -> Self {
        Self(Duration::from_secs(value))
    }
}

/// The interval after the last retry before retries reset.
#[derive(Debug, Deserialize, PartialEq, Clone, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_BACKOFF_LAST_RETRY_INTERVAL)]
pub struct BackoffLastRetryInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);

impl BackoffLastRetryInterval {
    /// Builds an interval of the given number of seconds.
    pub fn from_secs(value: u64) -> Self {
        Self(Duration::from_secs(value))
    }
}

/// The maximum number of retries before giving up.
#[derive(Debug, Deserialize, PartialEq, Clone, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_BACKOFF_MAX_RETRIES)]
pub struct MaxRetries(usize);

/// Backoff strategy configuration controlling how failed executions are retried.
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(default)]
pub struct BackoffStrategyConfig {
    /// The kind of backoff applied between retries.
    #[serde(rename = "type")]
    pub backoff_type: TemplateableValue<BackoffStrategyType>,
    /// The delay before retrying.
    pub backoff_delay: TemplateableValue<BackoffDelay>,
    /// The maximum number of retries.
    pub max_retries: TemplateableValue<MaxRetries>,
    /// The interval after the last retry.
    pub last_retry_interval: TemplateableValue<BackoffLastRetryInterval>,
}

impl Templateable for BackoffStrategyConfig {
    type Output = rendered::BackoffStrategyConfig;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        let backoff_type = self.backoff_type.template_with(variables)?;
        let backoff_delay = self.backoff_delay.template_with(variables)?;
        let max_retries = self.max_retries.template_with(variables)?;
        let last_retry_interval = self.last_retry_interval.template_with(variables)?;

        let result = Self::Output {
            backoff_type,
            backoff_delay,
            max_retries,
            last_retry_interval,
        };
        Ok(result)
    }
}

/// The kind of backoff applied between retries.
#[derive(Debug, Deserialize, Default, PartialEq, Clone)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum BackoffStrategyType {
    /// A constant delay between retries.
    #[default]
    Fixed,
    /// A delay growing linearly with the number of retries.
    Linear,
    /// A delay growing exponentially with the number of retries.
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
