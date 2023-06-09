use serde::Deserialize;

type AgentName = Option<String>;
type CustomAgentType = String;

/// AgentType represents the type of an agent. We currently support the following agent types:
/// - `nr_infra_agent`: New Relic Infrastructure agent
/// - `nr_otel_collector`: New Relic Distribution for the OpenTelemetry Collector
/// - `custom_agent_type`: Custom agent type (e.g. a binary with its arguments)
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum AgentType {
    InfraAgent(AgentName),
    Nrdot(AgentName),
    Custom(CustomAgentType, AgentName),
}

impl<'de> Deserialize<'de> for AgentType {
    fn deserialize<D>(deserializer: D) -> Result<AgentType, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let parts: Vec<&str> = s.split('/').collect();
        match parts.len() {
            1 => Ok(agent_type(parts[0], None)),
            2 => Ok(agent_type(parts[0], Some(parts[1].to_string()))),
            _ => Err(serde::de::Error::custom(
                "`agents` items must be of the form `agent_type` or `agent_type/name`, where `agent_type` is one of `nr_infra_agent`, `nr_otel_collector` or some other custom string, and `name` is a custom name for the agent. Examples: `nr_infra_agent`, `nr_otel_collector/my_col`, `my_agent/agent1`",
            )),
        }
    }
}

fn agent_type(agent_type: &str, id: Option<String>) -> AgentType {
    match agent_type {
        "nr_infra_agent" => AgentType::InfraAgent(id),
        "nr_otel_collector" => AgentType::Nrdot(id),
        custom => AgentType::Custom(custom.to_string(), id),
    }
}
