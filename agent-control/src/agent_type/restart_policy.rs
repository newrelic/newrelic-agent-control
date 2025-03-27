use super::{definition::TemplateableValue, error::AgentTypeError};
use duration_str::deserialize_duration;
use serde::Deserialize;
use std::{str::FromStr, time::Duration};
use wrapper_with_default::WrapperWithDefault;

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

#[derive(Debug, Deserialize, PartialEq, Clone, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_BACKOFF_DELAY)]
pub struct BackoffDelay(#[serde(deserialize_with = "deserialize_duration")] Duration);

impl BackoffDelay {
    pub fn new(value: Duration) -> Self {
        Self(value)
    }

    pub fn from_secs(value: u64) -> Self {
        Self(Duration::from_secs(value))
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_BACKOFF_LAST_RETRY_INTERVAL)]
pub struct BackoffLastRetryInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);

impl BackoffLastRetryInterval {
    pub fn new(value: Duration) -> Self {
        Self(value)
    }

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

#[cfg(test)]
mod tests {
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
