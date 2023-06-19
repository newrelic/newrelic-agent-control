use std::{collections::HashMap, time::Duration};

use config::Value;
use serde::Deserialize;

use crate::supervisor::restart::{Backoff, BackoffStrategy, LAST_RETRY_INTERVAL};

use super::agent_type::AgentType;

/*
The structures below assume a config similar to the following:

```yaml
agents:
    nr_infra_agent:
        restart_policy:
            backoff_strategy:
                type: fixed
                backoff_delay: 1s
                max_retries: 3
                with_last_retry_interval: 30s
        config: {} # Some arbitrary values passed to the agent itself.
        # TODO: What should we do with `bin'/`args` for custom agents?
```
 */

/*
Default values for supervisor restarts
TODO: refine values with real executions
*/
const BACKOFF_DELAY: Duration = Duration::from_secs(2);
const BACKOFF_MAX_RETRIES: usize = 20;
const BACKOFF_LAST_RETRY_INTERVAL: Duration = Duration::from_secs(420);

/// MetaAgentConfig represents the configuration for the meta agent.
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct MetaAgentConfig {
    /// agents is a map of agent types to their specific configuration (if any).
    pub agents: HashMap<AgentType, AgentConfig>,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct AgentConfig {
    #[serde(default)]
    pub restart_policy: RestartPolicyConfig,
    pub config: Option<HashMap<String, Value>>,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct RestartPolicyConfig {
    #[serde(default)]
    pub backoff_strategy: BackoffStrategyConfig,
    pub restart_exit_codes: Vec<i32>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum BackoffStrategyConfig {
    None,
    Fixed(BackoffStrategyInner),
    Linear(BackoffStrategyInner),
    Exponential(BackoffStrategyInner),
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct BackoffStrategyInner {
    pub backoff_delay: Duration,
    pub max_retries: usize,
    #[serde(default = "default_last_retry_interval")]
    pub with_last_retry_interval: Duration,
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
        Self::Fixed(BackoffStrategyInner {
            backoff_delay: BACKOFF_DELAY,
            max_retries: BACKOFF_MAX_RETRIES,
            with_last_retry_interval: BACKOFF_LAST_RETRY_INTERVAL,
        })
    }
}

fn realize_backoff_config(i: &BackoffStrategyInner) -> Backoff {
    Backoff::new()
        .with_initial_delay(i.backoff_delay)
        .with_max_retries(i.max_retries)
        .with_last_retry_interval(i.with_last_retry_interval)
}

fn default_last_retry_interval() -> Duration {
    LAST_RETRY_INTERVAL
}
