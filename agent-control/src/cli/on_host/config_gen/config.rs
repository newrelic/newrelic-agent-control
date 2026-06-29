//! Contains the definition of the configuration to be generated

use crate::cli::common::proxy_config::ProxyConfig;
use serde::Serialize;
use std::collections::HashMap;

/// Configuration to be written as result of the corresponding command.
#[derive(Debug, PartialEq, Serialize)]
pub struct Config {
    /// Fleet Control configuration, present only when Fleet Control is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fleet_control: Option<FleetControl>,

    /// Local server configuration.
    pub server: Server,

    /// Proxy configuration, omitted when empty.
    #[serde(skip_serializing_if = "is_none_or_empty")]
    pub proxy: Option<ProxyConfig>,

    /// Sub-agents to run, keyed by agent ID.
    pub agents: HashMap<String, Agent>,

    /// Logging configuration, when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<LogConfig>,
}

/// Fleet Control section of the generated configuration.
#[derive(Debug, PartialEq, Serialize)]
pub struct FleetControl {
    /// OpAMP endpoint used to communicate with Fleet Control.
    pub endpoint: String,
    /// Configuration for validating remote configuration signatures.
    pub signature_validation: SignatureValidation,
    /// Identifier of the fleet this instance belongs to.
    pub fleet_id: String,
    /// Authentication configuration for Fleet Control.
    pub auth_config: AuthConfig,
}

/// Signature validation settings for remote configuration.
#[derive(Debug, PartialEq, Serialize)]
pub struct SignatureValidation {
    /// URL of the public-key (JWKS) server used to verify signatures.
    pub public_key_server_url: String,
}

/// Authentication settings used to obtain Fleet Control tokens.
#[derive(Debug, PartialEq, Serialize)]
pub struct AuthConfig {
    /// Token renewal endpoint.
    pub token_url: String,
    /// Client ID of the system identity.
    pub client_id: String,
    /// Token provider name.
    pub provider: String,
    /// Path to the system identity's private key.
    pub private_key_path: String,
}

/// Local server section of the generated configuration.
#[derive(Debug, PartialEq, Serialize)]
pub struct Server {
    /// Whether the local server is enabled.
    pub enabled: bool,
}

/// A single sub-agent entry in the generated configuration.
#[derive(Debug, PartialEq, Serialize)]
pub struct Agent {
    /// Agent type identifier (e.g. `newrelic/com.newrelic.infrastructure:0.1.0`).
    pub agent_type: String,
}

/// Logging section of the generated configuration.
#[derive(Debug, PartialEq, Serialize)]
pub struct LogConfig {
    /// File logging configuration, when set.
    pub file: Option<FileLogConfig>,
}
/// File logging configuration.
#[derive(Debug, PartialEq, Serialize)]
pub struct FileLogConfig {
    /// Whether logging to file is enabled.
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
