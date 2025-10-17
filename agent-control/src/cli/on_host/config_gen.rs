//! Implementation of the generate-config command for the on-host cli.

use std::path::PathBuf;

use tracing::info;

use crate::{
    cli::{
        error::CliError,
        on_host::config_gen::{
            config::{AgentSet, AuthConfig, Config, FleetControl, Server, SignatureValidation},
            identity::{Identity, provide_identity},
            region::{Region, region_parser},
        },
    },
    http::config::ProxyConfig,
};

pub mod config;
pub mod identity;
pub mod region;

/// Generates the Agent Control configuration for host environments.
#[derive(Debug, clap::Parser)]
pub struct Args {
    /// Sets where the generated configuration should be written to.
    #[arg(long)]
    output_path: PathBuf,

    /// Defines if Fleet Control is enabled
    #[arg(long, default_value = "true")]
    fleet_enabled: bool,

    /// New Relic region
    #[arg(long, value_parser = region_parser())]
    region: Region,

    /// Fleet identifier
    #[arg(long)]
    fleet_id: String,

    /// Organization identifier
    #[arg(long)]
    organization_id: String,

    /// Set of agents to be used as local configuration.
    #[arg(long)]
    agent_set: AgentSet,

    /// Client ID corresponding to the parent system identity (requires `auth_client_secret`).
    #[arg(long)]
    auth_parent_client_id: String,

    /// Client Secret corresponding to the parent system identity (requires `auth_client_id`).
    #[arg(long)]
    auth_parent_client_secret: String,

    /// Auth token corresponding to the parent system identity.
    #[arg(long)]
    auth_parent_token: String,

    /// When (`auth_token` or `auth_client_id` + `auth_client_secret`) are set, this path is used
    /// to store the identity key. Otherwise, the path is expected to contain the already provided
    /// private key was already provided.
    #[arg(long)]
    auth_private_key_path: PathBuf,

    /// Client identifier corresponding to an already provisioned identity. No identity creation is performed,
    /// therefore setting this up also requires an existing private key pointed in `auth_private_key_path`.
    #[arg(long)]
    auth_client_id: String,

    /// Proxy configuration
    #[command(flatten)]
    proxy_config: Option<ProxyConfig>,
}

/// Generates the Agent Control configuration and any requisite according to the provided inputs.
pub fn generate_config(args: Args) -> Result<(), CliError> {
    info!("Generating Agent Control configuration");
    let yaml = gen_config(&args, provide_identity)?;

    std::fs::write(&args.output_path, yaml).map_err(|err| {
        CliError::Command(format!(
            "error writing the configuration file to '{}': {}",
            args.output_path.to_string_lossy(),
            err
        ))
    })?;
    info!(config_path=%args.output_path.to_string_lossy(), "Agent Control configuration generated successfully");
    Ok(())
}

/// Generates the configuration according to args using the provided function to generate the identity.
fn gen_config<F>(args: &Args, provide_identity_fn: F) -> Result<String, CliError>
where
    F: Fn(&Args) -> Result<Identity, CliError>,
{
    let fleet_control = if !args.fleet_enabled {
        None
    } else {
        let Identity {
            client_id,
            private_key_path,
        } = provide_identity_fn(args)?;

        Some(FleetControl {
            endpoint: args.region.opamp_endpoint().to_string(),
            signature_validation: SignatureValidation {
                public_key_server_url: args.region.public_key_endpoint().to_string(),
            },
            fleet_id: args.fleet_id.to_string(),
            auth_config: AuthConfig {
                token_url: args.region.token_renewal_endpoint().to_string(),
                client_id,
                provider: "local".to_string(),
                private_key_path: private_key_path.to_string_lossy().to_string(),
            },
        })
    };

    let config = Config {
        fleet_control,
        server: Server { enabled: true },
        proxy: args.proxy_config.clone(),
        agents: args.agent_set.into(),
    };

    serde_yaml::to_string(&config)
        .map_err(|err| CliError::Command(format!("failed to serialize configuration: {err}")))
}

#[cfg(test)]
mod tests {
    use crate::agent_control::config::AgentControlConfig;

    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(true, Region::US, AgentSet::InfraAgent, None, EXPECTED_INFRA_US)]
    #[case(true, Region::EU, AgentSet::Otel, None, EXPECTED_OTEL_EU)]
    #[case(true, Region::STAGING, AgentSet::None, None, EXPECTED_NONE_STAGING)]
    #[case(
        false,
        Region::US,
        AgentSet::InfraAgent,
        None,
        EXPECTED_FLEET_DISABLED_INFRA
    )]
    #[case(
        true,
        Region::US,
        AgentSet::InfraAgent,
        some_proxy_config(),
        EXPECTED_INFRA_US_PROXY
    )]
    fn test_gen_config_with_fleet_enabled(
        #[case] fleet_enabled: bool,
        #[case] region: Region,
        #[case] agent_set: AgentSet,
        #[case] proxy_config: Option<ProxyConfig>,
        #[case] expected: &str,
    ) {
        let args = create_test_args(fleet_enabled, region, agent_set, proxy_config);

        let yaml = gen_config(&args, identity_provider_mock).expect("result expected to be OK");

        // Check that the config can be used in Agent Control
        let _: AgentControlConfig =
            serde_yaml::from_str(&yaml).expect("Config should be valid for Agent Control");

        // Compare obtained config and expected
        let parsed: serde_yaml::Value =
            serde_yaml::from_str(&yaml).expect("Invalid generated YAML");
        let expected_parsed: serde_yaml::Value =
            serde_yaml::from_str(expected).expect("Invalid expectation");
        assert_eq!(parsed, expected_parsed);
    }

    fn identity_provider_mock(_: &Args) -> Result<Identity, CliError> {
        Ok(Identity {
            client_id: "test-client-id".to_string(),
            private_key_path: PathBuf::from("/path/to/private/key"),
        })
    }

    fn create_test_args(
        fleet_enabled: bool,
        region: Region,
        agent_set: AgentSet,
        proxy_config: Option<ProxyConfig>,
    ) -> Args {
        Args {
            output_path: PathBuf::from("/tmp/config.yaml"),
            fleet_enabled,
            region,
            fleet_id: "test-fleet-id".to_string(),
            organization_id: "test-org-id".to_string(),
            agent_set,
            auth_parent_client_id: "parent-client-id".to_string(),
            auth_parent_client_secret: "parent-client-secret".to_string(),
            auth_parent_token: "parent-token".to_string(),
            auth_private_key_path: PathBuf::from("/path/to/key"),
            auth_client_id: "client-id".to_string(),
            proxy_config,
        }
    }

    fn some_proxy_config() -> Option<ProxyConfig> {
        let proxy_config: ProxyConfig = serde_yaml::from_str(
            r#"{"url": "https://some.proxy.url/", "ca_bundle_dir": "/test/bundle/dir",
                "ca_bundle_file": "/test/bundle/file", "ignore_system_proxy": true}"#,
        )
        .unwrap();
        Some(proxy_config)
    }

    const EXPECTED_INFRA_US: &str = r#"
fleet_control:
  endpoint: https://opamp.service.newrelic.com/v1/opamp
  signature_validation:
    public_key_server_url: https://publickeys.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json
  fleet_id: test-fleet-id
  auth_config:
    token_url: https://system-identity-oauth.service.newrelic.com/oauth2/token
    client_id: test-client-id
    provider: local
    private_key_path: /path/to/private/key

server:
  enabled: true

agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
"#;

    const EXPECTED_OTEL_EU: &str = r#"
fleet_control:
  endpoint: https://opamp.service.eu.newrelic.com/v1/opamp
  signature_validation:
    public_key_server_url: https://publickeys.eu.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json
  fleet_id: test-fleet-id
  auth_config:
    token_url: https://system-identity-oauth.service.newrelic.com/oauth2/token
    client_id: test-client-id
    provider: local
    private_key_path: /path/to/private/key

server:
  enabled: true

agents:
  nrdot:
    agent_type: "newrelic/com.newrelic.opentelemetry.collector:0.1.0"
"#;

    const EXPECTED_NONE_STAGING: &str = r#"
fleet_control:
  endpoint: https://staging-service.newrelic.com/v1/opamp
  signature_validation:
    public_key_server_url: https://staging-publickeys.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json
  fleet_id: test-fleet-id
  auth_config:
    token_url: https://system-identity-oauth.staging-service.newrelic.com/oauth2/token
    client_id: test-client-id
    provider: local
    private_key_path: /path/to/private/key

server:
  enabled: true

agents: {}
"#;

    const EXPECTED_FLEET_DISABLED_INFRA: &str = r#"
server:
  enabled: true

agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
"#;

    const EXPECTED_INFRA_US_PROXY: &str = r#"
fleet_control:
  endpoint: https://opamp.service.newrelic.com/v1/opamp
  signature_validation:
    public_key_server_url: https://publickeys.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json
  fleet_id: test-fleet-id
  auth_config:
    token_url: https://system-identity-oauth.service.newrelic.com/oauth2/token
    client_id: test-client-id
    provider: local
    private_key_path: /path/to/private/key

server:
  enabled: true

proxy:
  url: https://some.proxy.url/
  ca_bundle_dir: /test/bundle/dir
  ca_bundle_file: /test/bundle/file
  ignore_system_proxy: true

agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
"#;
}
