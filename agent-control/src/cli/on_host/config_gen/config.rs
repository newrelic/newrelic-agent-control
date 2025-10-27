//! Contains the definition of the configuration to be generated

use std::{collections::HashMap, convert::Infallible};

use serde::Serialize;

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

/// Holds the proxy configuration.
/// Cannot use [crate::http::config::ProxyConfig] directly due lack of support for defaults in clap.
/// See <https://github.com/clap-rs/clap/issues/4746> for details.
#[derive(Debug, Default, Clone, PartialEq, Serialize, clap::Args)]
pub struct ProxyConfig {
    #[serde(skip_serializing_if = "is_none_or_empty_string", rename = "url")]
    #[arg(long, required = false)]
    pub proxy_url: Option<String>,

    #[serde(
        skip_serializing_if = "is_none_or_empty_string",
        rename = "ca_bundle_dir"
    )]
    #[arg(long, required = false)]
    pub proxy_ca_bundle_dir: Option<String>,

    #[serde(
        skip_serializing_if = "is_none_or_empty_string",
        rename = "ca_bundle_file"
    )]
    #[arg(long, required = false)]
    pub proxy_ca_bundle_file: Option<String>,

    #[arg(long, default_value_t = false, value_parser = ignore_system_proxy_parser, action = clap::ArgAction::Set)]
    pub ignore_system_proxy: bool,
}

// Helper to avoid serializing empty values
fn is_none_or_empty_string(v: &Option<String>) -> bool {
    v.as_ref().map(|s| s.is_empty()).unwrap_or(true)
}

// Custom parser to allow empty values as false booleans
fn ignore_system_proxy_parser(s: &str) -> Result<bool, Infallible> {
    match s.to_lowercase().as_str() {
        "" | "false" => Ok(false),
        _ => Ok(true),
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
    use clap::{Args, FromArgMatches};
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

    #[test]
    fn test_serialize_proxy_config_all_empty_options() {
        let proxy_config = ProxyConfig {
            proxy_url: Some(String::new()),
            proxy_ca_bundle_dir: Some(String::new()),
            proxy_ca_bundle_file: Some(String::new()),
            ignore_system_proxy: false,
        };

        let serialized = serde_yaml::to_string(&proxy_config).unwrap();
        // Only ignore_system_proxy should be present
        assert_eq!(serialized.trim(), "ignore_system_proxy: false");
    }

    #[test]
    fn test_serialize_proxy_config_none_options() {
        let proxy_config = ProxyConfig {
            proxy_url: None,
            proxy_ca_bundle_dir: None,
            proxy_ca_bundle_file: None,
            ignore_system_proxy: true,
        };

        let serialized = serde_yaml::to_string(&proxy_config).unwrap();
        // Only ignore_system_proxy should be present
        assert_eq!(serialized.trim(), "ignore_system_proxy: true");
    }

    #[rstest]
    #[case("", ProxyConfig::default())]
    #[case(
        "--proxy-url https://proxy.url --proxy-ca-bundle-dir=/bundle/dir --proxy-ca-bundle-file=/bundle/file --ignore-system-proxy true",
        ProxyConfig{proxy_url: Some("https://proxy.url".into()), proxy_ca_bundle_dir: Some("/bundle/dir".into()), proxy_ca_bundle_file: Some("/bundle/file".into()), ignore_system_proxy: true},
    )]
    #[case("--proxy-url= --proxy-ca-bundle-dir= --proxy-ca-bundle-file= --ignore-system-proxy=", ProxyConfig{proxy_url: Some("".into()), proxy_ca_bundle_dir: Some("".into()), proxy_ca_bundle_file: Some("".into()), ignore_system_proxy: false})]
    #[case(" --ignore-system-proxy=", ProxyConfig{ignore_system_proxy: false, ..Default::default()})]
    #[case(" --ignore-system-proxy=false", ProxyConfig{ignore_system_proxy: false, ..Default::default()})]
    #[case(" --ignore-system-proxy=true", ProxyConfig{ignore_system_proxy: true, ..Default::default()})]
    #[case(" --ignore-system-proxy=False", ProxyConfig{ignore_system_proxy: false, ..Default::default()})]
    #[case(" --ignore-system-proxy=True", ProxyConfig{ignore_system_proxy: true, ..Default::default()})]
    fn test_proxy_args(#[case] args: &str, #[case] expected: ProxyConfig) {
        let cmd = clap::Command::new("test").no_binary_name(true);
        let cmd = ProxyConfig::augment_args(cmd);
        let matches = cmd
            .try_get_matches_from(args.split_ascii_whitespace())
            .expect("arguments should be valid");
        let value = ProxyConfig::from_arg_matches(&matches).expect("should create the struct back");
        assert_eq!(value, expected)
    }
}
