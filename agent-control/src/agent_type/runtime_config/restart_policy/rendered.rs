use crate::agent_type::runtime_config::restart_policy::{
    BackoffDelay, BackoffLastRetryInterval, BackoffStrategyType, MaxRetries,
};

#[derive(Debug, PartialEq, Clone, Default)]
pub struct RestartPolicyConfig {
    /// Strategy configuration to retry in case of failure.
    pub backoff_strategy: BackoffStrategyConfig,
    /// List of exit codes that triggers a restart.
    pub restart_exit_codes: Vec<i32>,
}

#[derive(Debug, Default, PartialEq, Clone)]
pub struct BackoffStrategyConfig {
    pub backoff_type: BackoffStrategyType,
    pub backoff_delay: BackoffDelay,
    pub max_retries: MaxRetries,
    pub last_retry_interval: BackoffLastRetryInterval,
}
