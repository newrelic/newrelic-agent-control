use std::{marker::PhantomData, str::FromStr, time::Duration};

use crate::sub_agent::restart_policy::{Backoff, BackoffStrategy, RestartPolicy};
use duration_str::deserialize_duration;
use serde::Deserialize;

use super::{definition::TemplateableValue, error::AgentTypeError};

#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct RestartPolicyConfig {
    #[serde(default)]
    pub backoff_strategy: BackoffStrategyConfig,
    #[serde(default)]
    pub restart_exit_codes: Vec<i32>,
}

/*
Default values for supervisor restarts
TODO: refine values with real executions
*/
pub(super) const BACKOFF_DELAY: Duration = Duration::from_secs(2);
pub(super) const BACKOFF_MAX_RETRIES: usize = 0;
pub(super) const BACKOFF_LAST_RETRY_INTERVAL: Duration = Duration::from_secs(600);

#[derive(Debug, PartialEq, Clone)]
pub(super) struct BODelay;
#[derive(Debug, PartialEq, Clone)]
pub(super) struct BOLastRetryInterval;

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct BackoffDuration<T> {
    #[serde(deserialize_with = "deserialize_duration")]
    duration: Duration,
    #[serde(skip)]
    _duration_type: PhantomData<T>,
}

pub(super) type BackoffDelay = BackoffDuration<BODelay>;
pub(super) type BackoffLastRetryInterval = BackoffDuration<BOLastRetryInterval>;

#[cfg(test)]
impl<T> BackoffDuration<T> {
    pub fn new(value: Duration) -> Self {
        Self {
            duration: value,
            _duration_type: PhantomData,
        }
    }

    pub fn from_secs(value: u64) -> Self {
        Self {
            duration: Duration::from_secs(value),
            _duration_type: PhantomData,
        }
    }
}

impl Default for BackoffDuration<BODelay> {
    fn default() -> Self {
        Self {
            duration: BACKOFF_DELAY,
            _duration_type: PhantomData,
        }
    }
}

impl Default for BackoffDuration<BOLastRetryInterval> {
    fn default() -> Self {
        Self {
            duration: BACKOFF_LAST_RETRY_INTERVAL,
            _duration_type: PhantomData,
        }
    }
}

impl<T> From<Duration> for BackoffDuration<T> {
    fn from(value: Duration) -> Self {
        Self {
            duration: value,
            _duration_type: PhantomData,
        }
    }
}

impl<T> From<BackoffDuration<T>> for Duration {
    fn from(value: BackoffDuration<T>) -> Self {
        value.duration
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(default)]
pub struct BackoffStrategyConfig {
    #[serde(rename = "type")]
    pub backoff_type: TemplateableValue<BackoffStrategyType>,
    pub(super) backoff_delay: TemplateableValue<BackoffDelay>,
    pub max_retries: TemplateableValue<usize>,
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
            backoff_delay: TemplateableValue::new(BACKOFF_DELAY.into()),
            max_retries: TemplateableValue::new(BACKOFF_MAX_RETRIES),
            last_retry_interval: TemplateableValue::new(BACKOFF_LAST_RETRY_INTERVAL.into()),
        }
    }
}

fn realize_backoff_config(i: &BackoffStrategyConfig) -> Backoff {
    Backoff::new()
        .with_initial_delay(i.backoff_delay.clone().get().into())
        .with_max_retries(i.max_retries.clone().get())
        .with_last_retry_interval(i.last_retry_interval.clone().get().into())
}

#[cfg(test)]
mod test {
    use crate::agent_type::definition::TemplateableValue;

    use super::{BackoffStrategyConfig, BackoffStrategyType};

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
