use std::time::Duration;

use serde::Deserialize;

use crate::supervisor::restart::{Backoff, BackoffStrategy};

use super::agent_types::TemplateableValue;

#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct RestartPolicyConfig {
    #[serde(default)]
    pub backoff_strategy: BackoffStrategyConfig,
    #[serde(default)]
    pub restart_exit_codes: Vec<i32>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum BackoffStrategyConfig {
    None, // TODO: make it templateable somehow
    Fixed(BackoffStrategyInner),
    Linear(BackoffStrategyInner),
    Exponential(BackoffStrategyInner),
}

/*
Default values for supervisor restarts
TODO: refine values with real executions
*/
const BACKOFF_DELAY: Duration = Duration::from_secs(2);
const BACKOFF_MAX_RETRIES: usize = 0;
const BACKOFF_LAST_RETRY_INTERVAL: Duration = Duration::from_secs(600);

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct BackoffDuration(Duration);

impl BackoffDuration {
    pub fn new(value: Duration) -> Self {
        Self(value)
    }

    pub fn from_secs(value: u64) -> Self {
        Self(Duration::from_secs(value))
    }
}

impl From<Duration> for BackoffDuration {
    fn from(value: Duration) -> Self {
        Self(value)
    }
}

impl From<BackoffDuration> for Duration {
    fn from(value: BackoffDuration) -> Self {
        value.0
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(default)]
pub struct BackoffStrategyInner {
    pub backoff_delay_seconds: TemplateableValue<BackoffDuration>,
    pub max_retries: TemplateableValue<usize>,
    pub last_retry_interval_seconds: TemplateableValue<BackoffDuration>,
}

impl From<&BackoffStrategyConfig> for BackoffStrategy {
    fn from(value: &BackoffStrategyConfig) -> Self {
        match value {
            BackoffStrategyConfig::Fixed(inner) => {
                BackoffStrategy::Fixed(realize_backoff_config(inner))
            }
            BackoffStrategyConfig::Linear(inner) => {
                BackoffStrategy::Linear(realize_backoff_config(inner))
            }
            BackoffStrategyConfig::Exponential(inner) => {
                BackoffStrategy::Exponential(realize_backoff_config(inner))
            }
            BackoffStrategyConfig::None => BackoffStrategy::None,
        }
    }
}

impl Default for BackoffStrategyConfig {
    fn default() -> Self {
        Self::Linear(BackoffStrategyInner::default())
    }
}

impl Default for BackoffStrategyInner {
    fn default() -> Self {
        Self {
            backoff_delay_seconds: TemplateableValue::new(BACKOFF_DELAY.into()),
            max_retries: TemplateableValue::new(BACKOFF_MAX_RETRIES),
            last_retry_interval_seconds: TemplateableValue::new(BACKOFF_LAST_RETRY_INTERVAL.into()),
        }
    }
}

fn realize_backoff_config(i: &BackoffStrategyInner) -> Backoff {
    Backoff::new()
        .with_initial_delay(i.backoff_delay_seconds.clone().get().unwrap().into())
        .with_max_retries(i.max_retries.clone().get().unwrap())
        .with_last_retry_interval(i.last_retry_interval_seconds.clone().get().unwrap().into())
}
