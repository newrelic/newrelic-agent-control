use std::{str::FromStr, time::Duration};

use crate::sub_agent::restart_policy::{Backoff, BackoffStrategy, RestartPolicy};
use duration_str::deserialize_duration;
use serde::Deserialize;

use super::{definition::TemplateableValue, error::AgentTypeError};

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

/*
Default values for supervisor restarts
TODO: refine values with real executions
*/
pub(super) const DEFAULT_BACKOFF_DELAY: Duration = Duration::from_secs(2);
pub(super) const DEFAULT_BACKOFF_MAX_RETRIES: usize = 0;
pub(super) const DEFAULT_BACKOFF_LAST_RETRY_INTERVAL: Duration = Duration::from_secs(600);

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct BackoffDelay(#[serde(deserialize_with = "deserialize_duration")] Duration);

impl BackoffDelay {
    pub fn new(value: Duration) -> Self {
        Self(value)
    }

    pub fn from_secs(value: u64) -> Self {
        Self(Duration::from_secs(value))
    }
}

impl Default for BackoffDelay {
    fn default() -> Self {
        Self(DEFAULT_BACKOFF_DELAY)
    }
}

impl From<Duration> for BackoffDelay {
    fn from(value: Duration) -> Self {
        Self(value)
    }
}

impl From<BackoffDelay> for Duration {
    fn from(value: BackoffDelay) -> Self {
        value.0
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct BackoffLastRetryInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);

impl BackoffLastRetryInterval {
    pub fn new(value: Duration) -> Self {
        Self(value)
    }

    pub fn from_secs(value: u64) -> Self {
        Self(Duration::from_secs(value))
    }
}

impl Default for BackoffLastRetryInterval {
    fn default() -> Self {
        Self(DEFAULT_BACKOFF_LAST_RETRY_INTERVAL)
    }
}

impl From<Duration> for BackoffLastRetryInterval {
    fn from(value: Duration) -> Self {
        Self(value)
    }
}

impl From<BackoffLastRetryInterval> for Duration {
    fn from(value: BackoffLastRetryInterval) -> Self {
        value.0
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct MaxRetries(usize);

impl Default for MaxRetries {
    fn default() -> Self {
        Self(DEFAULT_BACKOFF_MAX_RETRIES)
    }
}

impl From<usize> for MaxRetries {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

impl From<MaxRetries> for usize {
    fn from(value: MaxRetries) -> Self {
        value.0
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(default)]
pub struct BackoffStrategyConfig {
    #[serde(rename = "type")]
    pub backoff_type: TemplateableValue<BackoffStrategyType>,
    pub(super) backoff_delay: TemplateableValue<BackoffDelay>,
    pub max_retries: TemplateableValue<MaxRetries>,
    pub(super) last_retry_interval: TemplateableValue<BackoffLastRetryInterval>,
}

impl BackoffStrategyConfig {
    pub(crate) fn are_values_in_sync_with_type(&self) -> bool {
        match self.backoff_type.clone().get() {
            BackoffStrategyType::None => {
                self.backoff_delay.is_template_empty()
                    && self.max_retries.is_template_empty()
                    && self.last_retry_interval.is_template_empty()
            }
            _ => true,
        }
    }
}

#[derive(Debug, Deserialize, Default, PartialEq, Clone)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum BackoffStrategyType {
    #[default]
    None,
    Fixed,
    Linear,
    Exponential,
}

impl FromStr for BackoffStrategyType {
    type Err = AgentTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(Self::None),
            "fixed" => Ok(Self::Fixed),
            "linear" => Ok(Self::Linear),
            "exponential" => Ok(Self::Exponential),
            _ => Err(AgentTypeError::UnknownBackoffStrategyType(s.to_string())),
        }
    }
}

impl From<RestartPolicyConfig> for RestartPolicy {
    fn from(value: RestartPolicyConfig) -> Self {
        RestartPolicy::new((&value.backoff_strategy).into(), value.restart_exit_codes)
    }
}

impl From<&BackoffStrategyConfig> for BackoffStrategy {
    fn from(value: &BackoffStrategyConfig) -> Self {
        match value.clone().backoff_type.get() {
            BackoffStrategyType::Fixed => BackoffStrategy::Fixed(realize_backoff_config(value)),
            BackoffStrategyType::Linear => BackoffStrategy::Linear(realize_backoff_config(value)),
            BackoffStrategyType::Exponential => {
                BackoffStrategy::Exponential(realize_backoff_config(value))
            }
            BackoffStrategyType::None => BackoffStrategy::None,
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

fn realize_backoff_config(i: &BackoffStrategyConfig) -> Backoff {
    Backoff::new()
        .with_initial_delay(i.backoff_delay.clone().get().into())
        .with_max_retries(i.max_retries.clone().get().into())
        .with_last_retry_interval(i.last_retry_interval.clone().get().into())
}

#[cfg(test)]
mod test {
    use super::{BackoffStrategyConfig, BackoffStrategyType};
    use crate::agent_type::definition::TemplateableValue;

    #[test]
    fn values_in_sync_with_type() {
        let strategy = BackoffStrategyConfig {
            backoff_type: TemplateableValue::new(BackoffStrategyType::None),
            backoff_delay: TemplateableValue::from_template("".to_string()),
            max_retries: TemplateableValue::from_template("".to_string()),
            last_retry_interval: TemplateableValue::from_template("".to_string()),
        };

        assert!(strategy.are_values_in_sync_with_type());

        let strategy = BackoffStrategyConfig {
            backoff_type: TemplateableValue::new(BackoffStrategyType::None),
            backoff_delay: TemplateableValue::from_template("".to_string()),
            max_retries: TemplateableValue::from_template("".to_string()),
            last_retry_interval: TemplateableValue::from_template("${something}".to_string()),
        };

        assert!(!strategy.are_values_in_sync_with_type());

        let strategy = BackoffStrategyConfig {
            backoff_type: TemplateableValue::new(BackoffStrategyType::Fixed),
            backoff_delay: TemplateableValue::from_template("".to_string()),
            max_retries: TemplateableValue::from_template("".to_string()),
            last_retry_interval: TemplateableValue::from_template("${something}".to_string()),
        };

        assert!(strategy.are_values_in_sync_with_type());
    }
}
