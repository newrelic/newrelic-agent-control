//! Contains the definition of the configuration to be generated

use std::collections::HashMap;

use serde::Serialize;

use crate::http::config::ProxyConfig;

/// Represents the set of agents to be included in the AC configuration.
#[derive(Debug, Copy, Clone, PartialEq, clap::ValueEnum)]
pub enum AgentSet {
    InfraAgent,
    Otel,
    None,
}

impl From<AgentSet> for HashMap<String, Agent> {
    fn from(value: AgentSet) -> Self {
        match value {
            AgentSet::InfraAgent => [(
                "nr-infra".to_string(),
                Agent {
                    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0".to_string(),
                },
            )]
            .into(),
            AgentSet::Otel => [(
                "nrdot".to_string(),
                Agent {
                    agent_type: "newrelic/com.newrelic.opentelemetry.collector:0.1.0".to_string(),
                },
            )]
            .into(),
            AgentSet::None => HashMap::new(),
        }
    }
}

/// Configuration to be written as result of the corresponding command.
#[derive(Debug, PartialEq, Serialize)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fleet_control: Option<FleetControl>,

    pub server: Server,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProxyConfig>,

    pub agents: HashMap<String, Agent>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct FleetControl {
    pub endpoint: String,
    pub signature_validation: SignatureValidation,
    pub fleet_id: String,
    pub auth_config: AuthConfig,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct SignatureValidation {
    pub public_key_server_url: String,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct AuthConfig {
    pub token_url: String,
    pub client_id: String,
    pub provider: String,
    pub private_key_path: String,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Server {
    pub enabled: bool,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Agent {
    pub agent_type: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(AgentSet::InfraAgent, vec![("nr-infra", "newrelic/com.newrelic.infrastructure:0.1.0")])]
    #[case(AgentSet::Otel, vec![("nrdot", "newrelic/com.newrelic.opentelemetry.collector:0.1.0")])]
    #[case(AgentSet::None, vec![])]
    fn test_agent_set_to_hash_map(
        #[case] agent_set: AgentSet,
        #[case] expected: Vec<(&str, &str)>,
    ) {
        let result: HashMap<String, Agent> = agent_set.into();
        let expected_map: HashMap<String, Agent> = expected
            .into_iter()
            .map(|(key, agent_type)| {
                (
                    key.to_string(),
                    Agent {
                        agent_type: agent_type.to_string(),
                    },
                )
            })
            .collect();

        assert_eq!(result, expected_map);
    }
}
