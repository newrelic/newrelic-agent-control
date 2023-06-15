use serde::Deserialize;

type AgentName = Option<String>;

/// AgentType represents the type of an agent. We currently support the following agent types:
/// - `nr_infra_agent`: New Relic Infrastructure agent
/// - `nr_otel_collector`: New Relic Distribution for the OpenTelemetry Collector
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum AgentType {
    InfraAgent(AgentName),
    Nrdot(AgentName),
}

impl<'de> Deserialize<'de> for AgentType {
    fn deserialize<D>(deserializer: D) -> Result<AgentType, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let parts: Vec<&str> = s.split('/').collect();
        match parts.len() {
            1 => agent_type(parts[0], None),
            2 => agent_type(parts[0], Some(parts[1].to_string())),
            _ => Err(serde::de::Error::custom(
                "`agents` items must be of the form `agent_type` or `agent_type/name`, where `agent_type` is one of `nr_infra_agent`, `nr_otel_collector` or some other custom string, and `name` is a custom name for the agent. Examples: `nr_infra_agent`, `nr_otel_collector/my_col`, `my_agent/agent1`",
            )),
        }
    }
}

fn agent_type<D>(agent_type: &str, id: Option<String>) -> Result<AgentType, D>
where
    D: serde::de::Error,
{
    match agent_type {
        "nr_infra_agent" => Ok(AgentType::InfraAgent(id)),
        "nr_otel_collector" => Ok(AgentType::Nrdot(id)),
        custom => Err(serde::de::Error::custom(format!(
            "unknown agent type {}",
            custom
        ))),
    }
}
