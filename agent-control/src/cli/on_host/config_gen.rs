//! Implementation of the generate-config command for the on-host cli.
use crate::cli::{
    error::CliError,
    on_host::{
        config_gen::{
            config::{
                AgentSet, AuthConfig, Config, FleetControl, LogConfig, Server, SignatureValidation,
            },
            identity::{Identity, provide_identity},
            region::{Region, region_parser},
        },
        proxy_config::ProxyConfig,
    },
};
use fs::file::{LocalFile, writer::FileWriter};
use std::{collections::HashMap, path::PathBuf};
use tracing::info;

pub mod config;
pub mod identity;
pub mod region;

pub const NR_LICENSE_ENV_VAR: &str = "NEW_RELIC_LICENSE_KEY";
const OTLP_ENDPOINT_ENV_VAR: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

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

    /// Sets the New Relic license key to be used for the Agents.
    #[arg(long, default_value_t)]
    newrelic_license_key: String,

    /// Path to a file containing environment variables to be set for the Agents.
    #[arg(long)]
    env_vars_file_path: Option<PathBuf>,
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

/// Generates:
/// 1. The Agent Control configuration file according to the provided args.
/// 2. The system identity required for Fleet Control, if applicable.
/// 3. The environment variables file required for the agents, if applicable.
pub fn generate(args: Args) -> Result<(), CliError> {
    write_config_and_generate_system_identity(&args)?;
    write_env_var_config(&args)?;
    Ok(())
}

/// Generates the Agent Control configuration, the system identity and any requisite according to the provided inputs.
fn write_config_and_generate_system_identity(args: &Args) -> Result<(), CliError> {
    info!("Generating Agent Control configuration");

    let yaml = generate_config_and_system_identity(args, provide_identity)?;

    LocalFile.write(&args.output_path, yaml).map_err(|err| {
        CliError::Command(format!(
            "error writing the configuration file to '{}': {}",
            args.output_path.to_string_lossy(),
            err
        ))
    })?;
    info!(config_path=%args.output_path.display(), "Agent Control configuration generated successfully");
    Ok(())
}

/// Generates and writes the environment variables configuration file if requested.
fn write_env_var_config(args: &Args) -> Result<(), CliError> {
    let Some(path) = &args.env_vars_file_path else {
        info!("No environment variables file path provided, skipping generation");
        return Ok(());
    };

    info!("Generating environment variables configuration");

    let yaml = generate_env_var_config(args)?;

    LocalFile.write(path, yaml).map_err(|err| {
        CliError::Command(format!(
            "error writing the environment variables file to '{}': {}",
            path.display(),
            err
        ))
    })?;

    info!(env_vars_path=%path.display(), "Environment variables file generated successfully");

    Ok(())
}

/// Generates the environment variables configuration according to the provided args.    
fn generate_env_var_config(args: &Args) -> Result<String, CliError> {
    info!("Inserting OTEL endpoint env var");
    let mut env_vars = HashMap::from([(
        OTLP_ENDPOINT_ENV_VAR.to_string(),
        args.region.otel_endpoint().to_string(),
    )]);

    if !args.newrelic_license_key.is_empty() {
        info!("Inserting New Relic license key env var");
        env_vars.insert(
            NR_LICENSE_ENV_VAR.to_string(),
            args.newrelic_license_key.clone(),
        );
    }

    serde_yaml::to_string(&env_vars).map_err(|err| {
        CliError::Command(format!(
            "failed to serialize environment variables configuration: {err}"
        ))
    })
}

/// Generates the configuration according to args using the provided function to generate the identity.
fn generate_config_and_system_identity<F>(
    args: &Args,
    provide_identity_fn: F,
) -> Result<String, CliError>
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
        log: default_log_config(),
    };

    serde_yaml::to_string(&config)
        .map_err(|err| CliError::Command(format!("failed to serialize configuration: {err}")))
}

fn default_log_config() -> Option<LogConfig> {
    #[cfg(target_family = "windows")]
    {
        Some(LogConfig {
            file: Some(config::FileLogConfig { enabled: true }),
        })
    }
    #[cfg(target_family = "unix")]
    {
        None
    }
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
                newrelic_license_key: Default::default(),
                env_vars_file_path: Default::default(),
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
    #[case(false, Region::US, AgentSet::InfraAgent, None, expected_infra_us())]
    #[case(false, Region::EU, AgentSet::Otel, None, expected_otel_eu())]
    #[case(
        false,
        Region::STAGING,
        AgentSet::NoAgents,
        None,
        expected_none_staging()
    )]
    #[case(
        true,
        Region::US,
        AgentSet::InfraAgent,
        None,
        expected_fleet_disabled_infra()
    )]
    #[case(
        false,
        Region::US,
        AgentSet::InfraAgent,
        some_proxy_config(),
        expected_infra_us_proxy()
    )]
    fn test_gen_config(
        #[case] fleet_enabled: bool,
        #[case] region: Region,
        #[case] agent_set: AgentSet,
        #[case] proxy_config: Option<ProxyConfig>,
        #[case] expected: String,
    ) {
        let args = create_test_args(fleet_enabled, region, agent_set, proxy_config);

        let yaml = generate_config_and_system_identity(&args, identity_provider_mock)
            .expect("result expected to be OK");

        // Check that the config can be used in Agent Control
        let _: AgentControlConfig =
            serde_yaml::from_str(&yaml).expect("Config should be valid for Agent Control");

        // Compare obtained config and expected
        let parsed: serde_yaml::Value =
            serde_yaml::from_str(&yaml).expect("Invalid generated YAML");
        let expected_parsed: serde_yaml::Value =
            serde_yaml::from_str(&expected).expect("Invalid expectation");
        assert_eq!(parsed, expected_parsed);
    }

    #[rstest]
    #[case(Region::US, "test-license")]
    #[case(Region::EU, "")]
    #[case(Region::STAGING, "another-license")]
    fn test_generate_env_var_config(#[case] region: Region, #[case] license: &str) {
        let args = Args {
            region,
            newrelic_license_key: license.to_string(),
            ..Default::default()
        };

        let yaml = generate_env_var_config(&args).expect("should generate env var config");

        let parsed: std::collections::HashMap<String, String> =
            serde_yaml::from_str(&yaml).expect("YAML should parse to a map");

        // Always contains the OTEL endpoint env var
        assert_eq!(
            parsed.get(OTLP_ENDPOINT_ENV_VAR),
            Some(&region.otel_endpoint().to_string())
        );

        if !license.is_empty() {
            // License key must be present and match
            assert_eq!(parsed.get(NR_LICENSE_ENV_VAR), Some(&license.to_string()));
            assert_eq!(
                parsed.len(),
                2,
                "only OTEL endpoint and license key expected"
            );
        } else {
            // License key must be absent
            assert!(!parsed.contains_key(NR_LICENSE_ENV_VAR));
            assert_eq!(parsed.len(), 1, "only OTEL endpoint expected");
        }
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
            newrelic_license_key: "test-license-key".to_string(),
            env_vars_file_path: None,
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

    // Reusable YAML sections
    const FLEET_CONTROL_US: &str = r#"
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

"#;

    const FLEET_CONTROL_EU: &str = r#"
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

"#;

    const FLEET_CONTROL_STAGING: &str = r#"
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

"#;

    const SERVER_SECTION: &str = r#"
server:
  enabled: true
"#;

    const PROXY_SECTION: &str = r#"
proxy:
  url: https://some.proxy.url/
  ca_bundle_dir: /test/bundle/dir
  ca_bundle_file: /test/bundle/file
  ignore_system_proxy: true
"#;

    const INFRA_AGENTS_SECTION: &str = r#"
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
"#;

    const OTEL_AGENTS_SECTION: &str = r#"
agents:
  nrdot:
    agent_type: "newrelic/com.newrelic.opentelemetry.collector:0.1.0"
"#;

    const NO_AGENTS_SECTION: &str = r#"agents: {}
"#;

    const LOG_SECTION: &str = r#"
log:
  file:
    enabled: true
"#;

    // Helper functions to compose expected configs
    fn log_section() -> String {
        if cfg!(target_family = "windows") {
            format!("\n{}", LOG_SECTION)
        } else {
            String::new()
        }
    }

    fn expected_infra_us() -> String {
        format!(
            "{}{}{}{}",
            FLEET_CONTROL_US,
            SERVER_SECTION,
            INFRA_AGENTS_SECTION,
            log_section()
        )
    }

    fn expected_otel_eu() -> String {
        format!(
            "{}{}{}{}",
            FLEET_CONTROL_EU,
            SERVER_SECTION,
            OTEL_AGENTS_SECTION,
            log_section()
        )
    }

    fn expected_none_staging() -> String {
        format!(
            "{}{}{}{}",
            FLEET_CONTROL_STAGING,
            SERVER_SECTION,
            NO_AGENTS_SECTION,
            log_section()
        )
    }

    fn expected_fleet_disabled_infra() -> String {
        format!(
            "{}{}{}",
            SERVER_SECTION,
            INFRA_AGENTS_SECTION,
            log_section()
        )
    }

    fn expected_infra_us_proxy() -> String {
        format!(
            "{}{}{}{}{}",
            FLEET_CONTROL_US,
            SERVER_SECTION,
            PROXY_SECTION,
            INFRA_AGENTS_SECTION,
            log_section()
        )
    }
}
