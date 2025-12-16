//! Implementation of the generate-config command for the on-host cli.
use crate::cli::{
    error::CliError,
    on_host::{
        config_gen::{
            config::{AgentSet, AuthConfig, Config, FleetControl, Server, SignatureValidation},
            identity::{Identity, provide_identity},
            region::{Region, region_parser},
        },
        proxy_config::ProxyConfig,
    },
};
use std::path::PathBuf;
use tracing::info;

pub mod config;
pub mod identity;
pub mod region;

/// Generates the Agent Control configuration for host environments.
#[derive(Debug, clap::Parser)]
pub struct Args {
    /// Sets where the generated configuration should be written to.
    #[arg(long, required = true)]
    output_path: PathBuf,

    /// Defines if Fleet Control is enabled
    #[arg(long, default_value_t = false)]
    fleet_disabled: bool,

    /// New Relic region
    #[arg(long, value_parser = region_parser(), required = true)]
    region: Region,

    /// Fleet identifier
    #[arg(long, default_value_t)]
    fleet_id: String,

    /// Set of agents to be used as local configuration.
    #[arg(long, required = true)]
    agent_set: AgentSet,

    /// Organization identifier
    #[arg(long, default_value_t)]
    organization_id: String,

    /// Client ID corresponding to the parent system identity (requires `auth_client_secret`).
    #[arg(long, default_value_t)]
    auth_parent_client_id: String,

    /// Client Secret corresponding to the parent system identity (requires `auth_client_id`).
    #[arg(long, default_value_t)]
    auth_parent_client_secret: String,

    /// Auth token corresponding to the parent system identity.
    #[arg(long, default_value_t)]
    auth_parent_token: String,

    /// When ('auth_token' or 'auth_client_id' + 'auth_client_secret') are set, this path is used
    /// to store the identity key. Otherwise, the path is expected to contain the already provided
    /// private key was already provided.
    #[arg(long)]
    auth_private_key_path: Option<PathBuf>,

    /// Client identifier corresponding to an already provisioned identity. No identity creation is performed,
    /// therefore setting this up also requires an existing private key pointed in `auth_private_key_path`.
    #[arg(long, default_value_t)]
    auth_client_id: String,

    /// Proxy configuration
    #[command(flatten)]
    proxy_config: Option<ProxyConfig>,
}

impl Args {
    /// Performs additional args validation (not covered by clap's arguments)
    pub fn validate(&self) -> Result<(), String> {
        if !self.fleet_disabled {
            // Fleet-id is required
            if self.fleet_id.is_empty() {
                return Err(String::from("'fleet_id' should be set when enabling fleet"));
            }
            // Any method to provide the identity should be selected
            if self.auth_client_id.is_empty()
                && self.auth_parent_token.is_empty()
                && self.auth_parent_client_secret.is_empty()
            {
                return Err(String::from(
                    "either 'auth_client_id', 'auth_parent_token' or 'auth_parent_secret' should be set when enabling fleet",
                ));
            }
            // 'auth_private_key_path' is required
            let Some(auth_private_key_path) = self.auth_private_key_path.as_ref() else {
                return Err(String::from(
                    "'auth_private_key_path' needs to be set when enabling fleet",
                ));
            };
            // Requirements for existing identity
            if !self.auth_client_id.is_empty() && !auth_private_key_path.exists() {
                return Err(String::from(
                    "when 'auth_client_id' is provided the 'auth_private_key_path' must also be provided and exist",
                ));
            }
            // Requirements for token-based identity generation
            if !self.auth_parent_token.is_empty()
                && (self.organization_id.is_empty() || self.auth_parent_client_id.is_empty())
            {
                return Err(String::from(
                    "token based system identity generation requires 'auth_parent_token', 'auth_parent_client_id' and 'organization_id'",
                ));
            }
            // Requirements for client + secret identity generation
            if !self.auth_parent_client_secret.is_empty()
                && (self.organization_id.is_empty() || self.auth_parent_client_id.is_empty())
            {
                return Err(String::from(
                    "client-secret based system identity generation requires 'auth_parent_client_secret', 'auth_parent_client_id' and 'organization_id'",
                ));
            }
        }
        if let Some(proxy_config) = self.proxy_config.clone()
            && let Err(err) = crate::http::config::ProxyConfig::try_from(proxy_config)
        {
            return Err(format!("invalid proxy configuration: {err}"));
        }
        Ok(())
    }
}

/// Generates the Agent Control configuration, the system identity and any requisite according to the provided inputs.
pub fn write_config_and_system_identity(args: Args) -> Result<(), CliError> {
    info!("Generating Agent Control configuration");

    let yaml = generate_config_and_system_identity(&args, provide_identity)?;

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
fn generate_config_and_system_identity<F>(
    args: &Args,
    provide_identity_fn: F,
) -> Result<Vec<u8>, CliError>
where
    F: Fn(&Args) -> Result<Identity, CliError>,
{
    let fleet_control = if args.fleet_disabled {
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

    let mut buffer = Vec::new();
    serde_yaml::to_writer(&mut buffer, &config)
        .map_err(|err| CliError::Command(format!("failed to serialize configuration: {err}")))?;
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::config::AgentControlConfig;
    use assert_matches::assert_matches;
    use clap::{CommandFactory, FromArgMatches};
    use rstest::rstest;
    use std::env::current_dir;

    impl Default for Args {
        fn default() -> Self {
            Args {
                output_path: Default::default(),
                fleet_disabled: false,
                region: Region::US,
                fleet_id: Default::default(),
                agent_set: AgentSet::NoAgents,
                organization_id: Default::default(),
                auth_parent_client_id: Default::default(),
                auth_parent_client_secret: Default::default(),
                auth_parent_token: Default::default(),
                auth_private_key_path: None,
                auth_client_id: Default::default(),
                proxy_config: None,
            }
        }
    }

    #[rstest]
    #[case::fleet_disabled(
        || String::from("--fleet-disabled --output-path /some/path --agent-set otel --region us")
    )]
    #[case::identity_already_provided(
        || format!("--output-path /some/path --agent-set otel --region us --fleet-id some-id --auth-private-key-path {} --auth-client-id some-client-id", pwd())
    )]
    #[case::token_based_identity(
        || format!("--output-path /some/path --agent-set otel --region us --fleet-id some-id --auth-private-key-path {} --auth-parent-token TOKEN --auth-parent-client-id id --organization-id org-id", pwd())
    )]
    #[case::client_id_and_secret_based_identity(
        || format!("--output-path /some/path --agent-set otel --region us --fleet-id some-id --auth-private-key-path {} --auth-parent-client-secret SECRET --auth-parent-client-id id --organization-id org-id", pwd())
    )]
    fn test_args_validation(#[case] args: fn() -> String) {
        let cmd = Args::command().no_binary_name(true);
        let matches = cmd
            .try_get_matches_from(args().split_ascii_whitespace())
            .expect("arguments should be valid");
        let args = Args::from_arg_matches(&matches).expect("should create the struct back");
        assert_matches!(args.validate(), Ok(_));
    }

    #[rstest]
    #[case::missing_identity_creation_method(
        || format!("--output-path /some/path --agent-set otel --region us --auth-private-key-path {}", pwd())
    )]
    #[case::missing_private_key_path(
        || String::from("--output-path /some/path --agent-set otel --region us --auth-client-id some-client-id")
    )]
    #[case::nonexisting_private_key_path(
        || String::from("--output-path /some/path --agent-set otel --region us --auth-client-id some-client-id --auth-private-key-path /do-not/exist")
    )]
    #[case::missing_auth_parent_client_id_with_token(
        || format!("--output-path /some/path --agent-set otel --region us --auth-private-key-path {} --auth-parent-token TOKEN --organization-id org-id", pwd())
    )]
    #[case::missing_org_id_with_token(
        || format!("--output-path /some/path --agent-set otel --region us --auth-private-key-path {} --auth-parent-token TOKEN --auth-parent-client-id id", pwd())
    )]
    #[case::missing_org_id_with_secret(
        || format!("--output-path /some/path --agent-set otel --region us --auth-private-key-path {} --auth-parent-client-secret SECRET --organization-id org-id", pwd())
    )]
    #[case::missing_auth_parent_client_id_with_secret(
        || format!("--output-path /some/path --agent-set otel --region us --auth-private-key-path {} --auth-parent-client-secret SECRET --auth-parent-client-id id", pwd())
    )]
    #[case::invalid_proxy_config(
        || String::from("--fleet-disabled --output-path /some/path --agent-set otel --region us --proxy-url https::/invalid")
    )]
    fn test_args_validation_errors(#[case] args: fn() -> String) {
        let cmd = Args::command().no_binary_name(true);
        let matches = cmd
            .try_get_matches_from(args().split_ascii_whitespace())
            .expect("arguments should be valid");
        let args = Args::from_arg_matches(&matches).expect("should create the struct back");

        assert_matches!(args.validate(), Err(_));
    }

    #[rstest]
    #[case(false, Region::US, AgentSet::InfraAgent, None, EXPECTED_INFRA_US)]
    #[case(false, Region::EU, AgentSet::Otel, None, EXPECTED_OTEL_EU)]
    #[case(
        false,
        Region::STAGING,
        AgentSet::NoAgents,
        None,
        EXPECTED_NONE_STAGING
    )]
    #[case(
        true,
        Region::US,
        AgentSet::InfraAgent,
        None,
        EXPECTED_FLEET_DISABLED_INFRA
    )]
    #[case(
        false,
        Region::US,
        AgentSet::InfraAgent,
        some_proxy_config(),
        EXPECTED_INFRA_US_PROXY
    )]
    fn test_gen_config(
        #[case] fleet_enabled: bool,
        #[case] region: Region,
        #[case] agent_set: AgentSet,
        #[case] proxy_config: Option<ProxyConfig>,
        #[case] expected: &str,
    ) {
        let args = create_test_args(fleet_enabled, region, agent_set, proxy_config);

        let yaml = String::from_utf8(
            generate_config_and_system_identity(&args, identity_provider_mock)
                .expect("result expected to be OK"),
        )
        .unwrap();

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
            fleet_disabled: fleet_enabled,
            region,
            fleet_id: "test-fleet-id".to_string(),
            organization_id: "test-org-id".to_string(),
            agent_set,
            auth_parent_client_id: "parent-client-id".to_string(),
            auth_parent_client_secret: "parent-client-secret".to_string(),
            auth_parent_token: "parent-token".to_string(),
            auth_private_key_path: Some(PathBuf::from("/path/to/key")),
            auth_client_id: "client-id".to_string(),
            proxy_config,
        }
    }

    fn pwd() -> String {
        current_dir().unwrap().to_string_lossy().to_string()
    }

    fn some_proxy_config() -> Option<ProxyConfig> {
        let proxy_config = ProxyConfig {
            proxy_url: Some("https://some.proxy.url/".to_string()),
            proxy_ca_bundle_dir: Some("/test/bundle/dir".to_string()),
            proxy_ca_bundle_file: Some("/test/bundle/file".to_string()),
            ignore_system_proxy: true,
        };
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
  endpoint: https://opamp.staging-service.newrelic.com/v1/opamp
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
