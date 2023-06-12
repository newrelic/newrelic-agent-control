use std::{collections::HashMap, time::Duration};

use config::Value;
use serde::Deserialize;

use crate::supervisor::restart::LAST_RETRY_INTERVAL;

use super::agent_type::AgentType;

/*
The structures below assume a config similar to the following:

```yaml
agents:
    nr_infra_agent:
        restart_policy:
            backoff_strategy: fixed
            backoff_delay: 1s
            max_retries: 3
            with_last_retry_interval: 30s
        config: {} # Some arbitrary values passed to the agent itself.
        # TODO: What should we do with `bin'/`args` for custom agents?
```
 */

/// MetaAgentConfig represents the configuration for the meta agent.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MetaAgentConfig {
    /// agents is a map of agent types to their specific configuration (if any).
    #[serde(deserialize_with = "des_agent_configs")]
    pub agents: HashMap<AgentType, Option<AgentConfig>>,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct AgentConfig {
    pub restart_policy: Option<RestartPolicy>,
    pub config: Option<HashMap<String, Value>>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct RestartPolicy {
    #[serde(default)]
    pub backoff_strategy: BackoffStrategy,
    pub backoff_delay: Duration,
    pub max_retries: usize,
    #[serde(default)]
    pub with_last_retry_interval: RetryInterval,
    pub restart_exit_codes: Vec<i32>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum BackoffStrategy {
    None,
    Fixed,
    Linear,
    Exponential,
}

impl Default for BackoffStrategy {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct RetryInterval(Duration);

impl Default for RetryInterval {
    fn default() -> Self {
        Self(LAST_RETRY_INTERVAL)
    }
}

fn des_agent_configs<'de, D>(
    deserializer: D,
) -> Result<HashMap<AgentType, Option<AgentConfig>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // let mut map = HashMap::new();
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
