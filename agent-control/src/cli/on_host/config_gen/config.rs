//! Contains the definition of the configuration to be generated

use std::collections::HashMap;

use serde::Serialize;

use crate::cli::on_host::proxy_config::ProxyConfig;

/// Represents the set of agents to be included in the AC configuration.
#[derive(Debug, Copy, Clone, PartialEq, clap::ValueEnum)]
pub enum AgentSet {
    InfraAgent,
    Otel,
    NoAgents,
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
            AgentSet::NoAgents => HashMap::new(),
        }
    }
}

/// Configuration to be written as result of the corresponding command.
#[derive(Debug, PartialEq, Serialize)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fleet_control: Option<FleetControl>,

    pub server: Server,

    #[serde(skip_serializing_if = "is_none_or_empty")]
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

/// Helper to avoid deserializing proxy values when empty or default
fn is_none_or_empty(v: &Option<ProxyConfig>) -> bool {
    v.as_ref().is_none_or(|v| v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(AgentSet::InfraAgent, vec![("nr-infra", "newrelic/com.newrelic.infrastructure:0.1.0")])]
    #[case(AgentSet::Otel, vec![("nrdot", "newrelic/com.newrelic.opentelemetry.collector:0.1.0")])]
    #[case(AgentSet::NoAgents, vec![])]
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

    #[rstest]
    #[case::no_proxy_config(
        Config {
            fleet_control: None,
            server: Server { enabled: true },
            proxy: None,
            agents: HashMap::new(),
        },
        r#"{"server":{"enabled":true},"agents":{}}"#
    )]
    #[case::empty_proxy_config(
        Config {
            fleet_control: None,
            server: Server { enabled: false },
            proxy: Some(ProxyConfig::default()),
            agents: HashMap::new(),
        },
        r#"{"server":{"enabled":false},"agents":{}}"#
    )]
    #[case::some_proxy_config(
        Config {
            fleet_control: None,
            server: Server { enabled: true },
            proxy: Some(ProxyConfig { proxy_url: Some("http://proxy:8080".to_string()), ..Default::default() }),
            agents: AgentSet::InfraAgent.into(),
        },
        r#"{"server":{"enabled":true},"proxy":{"url":"http://proxy:8080"},"agents":{"nr-infra":{"agent_type":"newrelic/com.newrelic.infrastructure:0.1.0"}}}"#
    )]
    fn test_config_serialization(#[case] config: Config, #[case] expected_json: &str) {
        let serialized = serde_json::to_string(&config).unwrap();
        assert_eq!(serialized, expected_json);
    }
}
