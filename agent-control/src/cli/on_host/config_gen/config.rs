//! Contains the definition of the configuration to be generated

use crate::cli::on_host::proxy_config::ProxyConfig;
use serde::Serialize;
use std::collections::HashMap;

/// Configuration to be written as result of the corresponding command.
#[derive(Debug, PartialEq, Serialize)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fleet_control: Option<FleetControl>,

    pub server: Server,

    #[serde(skip_serializing_if = "is_none_or_empty")]
    pub proxy: Option<ProxyConfig>,

    pub agents: HashMap<String, Agent>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<LogConfig>,
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

#[derive(Debug, PartialEq, Serialize)]
pub struct LogConfig {
    pub file: Option<FileLogConfig>,
}
#[derive(Debug, PartialEq, Serialize)]
pub struct FileLogConfig {
    pub enabled: bool,
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
    #[case::no_proxy_and_log_config(
        Config {
            fleet_control: None,
            server: Server { enabled: true },
            proxy: None,
            agents: HashMap::new(),
            log: None,
        },
        r#"{"server":{"enabled":true},"agents":{}}"#
    )]
    #[case::some_log_config(
        Config {
            fleet_control: None,
            server: Server { enabled: true },
            proxy: None,
            agents: HashMap::new(),
            log: Some(LogConfig { file: Some(FileLogConfig { enabled: false }) }),
        },
        r#"{"server":{"enabled":true},"agents":{},"log":{"file":{"enabled":false}}}"#
    )]
    #[case::empty_proxy_config(
        Config {
            fleet_control: None,
            server: Server { enabled: false },
            proxy: Some(ProxyConfig::default()),
            agents: HashMap::new(),
            log: None,
        },
        r#"{"server":{"enabled":false},"agents":{}}"#
    )]
    #[case::some_proxy_config(
        Config {
            fleet_control: None,
            server: Server { enabled: true },
            proxy: Some(ProxyConfig { proxy_url: Some("http://proxy:8080".to_string()), ..Default::default() }),
            agents: [("nr-infra".to_string(), Agent { agent_type: "newrelic/com.newrelic.infrastructure:0.1.0".to_string() })].into_iter().collect(),
            log: None,
        },
        r#"{"server":{"enabled":true},"proxy":{"url":"http://proxy:8080"},"agents":{"nr-infra":{"agent_type":"newrelic/com.newrelic.infrastructure:0.1.0"}}}"#
    )]
    fn test_config_serialization(#[case] config: Config, #[case] expected_json: &str) {
        let serialized = serde_json::to_string(&config).unwrap();
        assert_eq!(serialized, expected_json);
    }
}
