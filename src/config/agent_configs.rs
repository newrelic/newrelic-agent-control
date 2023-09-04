use std::{collections::HashMap, fmt::Display, path::Path, time::Duration};
use serde::Deserialize;

/*
The structures below assume a config similar to the following:

```yaml
agents:
    nr_infra_agent:
        restart_policy:
            backoff_strategy:
                type: fixed
                backoff_delay_seconds: 3
                max_retries: 3
                last_retry_interval_seconds: hello
        config: {} # Some arbitrary values passed to the agent itself.
        # TODO: What should we do with `bin'/`args` for custom agents?

agents:
    nrdot_gw:
        type: newrelic/nrdot:0.1.1
        config: config-nrdot-gw.yaml
    nrdot_collector:
        type: newrelic/nrdot:0.1.1
        config: config-nrdot-collector.yaml

agent_type
namespace, name, version
->getFQN() -> namespace/name:verison

agent_config (variables file) no reference to type

supervisor_config
namespace/name:verison

Agent_type registry
namespace/name:version -> AgentType

```
 */

#[derive(Debug, Deserialize, PartialEq, Clone, Hash, Eq)]
pub struct AgentID(pub String);

impl AgentID {
    pub fn get(self) -> String {
        self.0
    }
}

impl Display for AgentID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

/// SuperAgentConfig represents the configuration for the super agent.
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct SuperAgentConfig {
    /// agents is a map of agent types to their specific configuration (if any).
    pub agents: HashMap<AgentID, AgentSupervisorConfig>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct AgentSupervisorConfig {
    pub agent_type: String, // FQN of the agent type, ex: newrelic/nrdot:0.1.0
    pub config_path: String,
}
