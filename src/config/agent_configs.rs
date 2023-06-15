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

/// MetaAgentConfig represents the configuration for the meta agent.
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct MetaAgentConfig {
    /// agents is a map of agent types to their specific configuration (if any).
    #[serde(deserialize_with = "des_agent_configs")]
    pub agents: HashMap<AgentType, Option<AgentConfig>>,
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
        Self::None
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

fn des_agent_configs<'de, D>(
    deserializer: D,
) -> Result<HashMap<AgentType, Option<AgentConfig>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let agents: HashMap<AgentType, Option<AgentConfig>> = HashMap::deserialize(deserializer)?;
    agents
        .into_iter()
        .try_fold(HashMap::new(), |mut acc, (agent_t, agent_cfg)| {
            if let AgentType::Custom(custom_type, custom_agent_name) = &agent_t {
                // Get custom agent type and name as it is in the config
                let agent_t_name =
                    agent_type_with_name(custom_type.as_ref(), custom_agent_name.as_ref());
                // If using a custom agent type, check that the config contains a `bin` key,
                // the minimum required info for a custom agent.
                let a_cfg = agent_cfg.as_ref().ok_or(serde::de::Error::custom(format!(
                    "custom agent `{}`'s spec must not be empty",
                    agent_t_name
                )))?;
                let cfg = a_cfg
                    .config
                    .as_ref()
                    .ok_or(serde::de::Error::custom(format!(
                        "custom agent {}'s `config` field must not be empty",
                        agent_t_name
                    )))?;
                if !cfg.contains_key("bin") {
                    Err(serde::de::Error::custom(format!(
                        "custom agent type `{}` must have a `bin` key",
                        agent_t_name
                    )))?
                }
            }
            acc.insert(agent_t, agent_cfg);
            Ok(acc)
        })
}

fn agent_type_with_name(agent_type: &str, agent_name: Option<&String>) -> String {
    match agent_name {
        Some(name) => format!("{}/{}", agent_type, name),
        None => agent_type.to_string(),
    }
}
