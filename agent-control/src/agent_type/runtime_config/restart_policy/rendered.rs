//! Restart policy configuration after templating.
use crate::agent_type::runtime_config::restart_policy::{
    BackoffDelay, BackoffLastRetryInterval, BackoffStrategyType, MaxRetries,
};

/// Rendered restart policy configuration.
#[derive(Debug, PartialEq, Clone, Default)]
pub struct RestartPolicyConfig {
    /// Strategy configuration to retry in case of failure.
    pub backoff_strategy: BackoffStrategyConfig,
}

/// Rendered backoff strategy configuration.
#[derive(Debug, Default, PartialEq, Clone)]
pub struct BackoffStrategyConfig {
    /// The kind of backoff applied between retries.
    pub backoff_type: BackoffStrategyType,
    /// The delay before retrying.
    pub backoff_delay: BackoffDelay,
    /// The maximum number of retries.
    pub max_retries: MaxRetries,
    /// The interval after the last retry.
    pub last_retry_interval: BackoffLastRetryInterval,
}
