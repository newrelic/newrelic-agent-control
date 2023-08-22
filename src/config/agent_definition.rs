use serde::Deserialize;

type AgentName = Option<String>;

/// AgentDefinition represents the type of an agent. We currently support the following agent types:
/// - `nr_infra_agent`: New Relic Infrastructure agent
/// - `nr_otel_collector`: New Relic Distribution for the OpenTelemetry Collector
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum AgentDefinition {
    InfraAgent(AgentName),
    Nrdot(AgentName),
}

// for debugging purposes
impl From<&AgentDefinition> for String {
    fn from(value: &AgentDefinition) -> Self {
        match value {
            crate::config::agent_definition::AgentDefinition::InfraAgent(name) => match name {
                Some(name) => format!("infra_agent/{}", name),
                None => "infra_agent".to_string(),
            },
            crate::config::agent_definition::AgentDefinition::Nrdot(name) => match name {
                Some(name) => format!("nr_otel_collector/{}", name),
                None => "nr_otel_collector".to_string(),
            },
        }
    }
}

impl<'de> Deserialize<'de> for AgentDefinition {
    fn deserialize<D>(deserializer: D) -> Result<AgentDefinition, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let parts: Vec<&str> = s.split('/').collect();
        match parts.len() {
            1 => agent_definition(parts[0], None),
            2 => agent_definition(parts[0], Some(parts[1].to_string())),
            _ => Err(serde::de::Error::custom(
                "`agents` items must be of the form `agent_definition` or `agent_definition/name`, where `agent_definition` is one of `nr_infra_agent` or `nr_otel_collector`, and `name` is a custom name for the agent. Examples: `nr_infra_agent`, `nr_otel_collector/my_col`",
            )),
        }
    }
}

fn agent_definition<D>(agent_definition: &str, id: Option<String>) -> Result<AgentDefinition, D>
where
    D: serde::de::Error,
{
    match agent_definition {
        "nr_infra_agent" => Ok(AgentDefinition::InfraAgent(id)),
        "nr_otel_collector" => Ok(AgentDefinition::Nrdot(id)),
        custom => Err(serde::de::Error::custom(format!(
            "unknown agent type {}",
            custom
        ))),
    }
}
