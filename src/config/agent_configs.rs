use std::collections::HashMap;

use config::Value;
use serde::Deserialize;

use super::agent_type::AgentType;

/// MetaAgentConfig represents the configuration for the meta agent.
#[derive(Debug, Deserialize, PartialEq)]
pub struct MetaAgentConfig {
    /// agents is a map of agent types to their specific configuration (if any).
    #[serde(deserialize_with = "des_agent_configs")]
    pub agents: HashMap<AgentType, Value>,
}

fn des_agent_configs<'de, D>(deserializer: D) -> Result<HashMap<AgentType, Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let mut map = HashMap::new();
    let kv: HashMap<AgentType, Value> = HashMap::deserialize(deserializer)?;
    for (agent_type, config_value) in kv {
        if let AgentType::Custom(custom_type, custom_agent_name) = &agent_type {
            // Get custom agent type and name as it is in the config
            let agent_type_name =
                agent_type_with_name(custom_type.as_ref(), custom_agent_name.as_ref());
            // Get the actual config values for the custom agent
            let config_values = config_value.clone().into_table().map_err(|err| {
                serde::de::Error::custom(format!(
                    "could not get config mapping for {}: {} ",
                    agent_type_name, err
                ))
            })?;
            // If using a custom agent type, check that the config contains a `bin` key,
            // the minimum required info for a custom agent.
            if !config_values.contains_key("bin") {
                return Err(serde::de::Error::custom(format!(
                    "custom agent type `{}` must have a `bin` key",
                    agent_type_name
                )));
            }
        }
        map.insert(agent_type, config_value);
    }
    Ok(map)
}

fn agent_type_with_name(agent_type: &str, agent_name: Option<&String>) -> String {
    match agent_name {
        Some(name) => format!("{}/{}", agent_type, name),
        None => agent_type.to_string(),
    }
}
