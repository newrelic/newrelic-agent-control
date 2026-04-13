//! Implementation of the generate-config command for the on-host cli.
use crate::cli::{
    common::{
        error::CliError,
        proxy_config::ProxyConfig,
        region::{Region, region_parser},
        system_identity::{ParentAuthMethod, ProvisionIdentityArgs, create_identity},
    },
    on_host::config_gen::config::{
        AuthConfig, Config, FleetControl, LogConfig, Server, SignatureValidation,
    },
};
use fs::file::{LocalFile, writer::FileWriter};
use nr_auth::key::{
    generation::{KeyType, PublicKeyPem},
    local::{LocalKeyPairGenerator, LocalKeyPairGeneratorConfig},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tracing::info;

pub mod config;

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

    /// Path where the private key is stored or will be written.
    #[arg(long)]
    auth_private_key_path: Option<PathBuf>,

    /// Client ID of an already-provisioned system identity. When non-empty, no identity
    /// generation is performed.
    #[arg(long, default_value_t)]
    auth_client_id: String,

    /// Identity configuration
    #[command(flatten)]
    identity: ProvisionIdentityArgs,

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

/// Valid data to create a SystemIdentity, represent [SystemIdentityArgs] after validation.
#[derive(Debug)]
pub struct SystemIdentitySpec {
    /// Data to get or create the System Identity to be used by Agent Control
    pub system_identity_data: SystemIdentityData,
    /// Path where the corresponding private key needs to be read from or written to
    pub private_key_path: PathBuf,
}

/// Defines whether a SystemIdentity already exists or needs to be provisioned
#[derive(Debug)]
pub enum SystemIdentityData {
    /// The Identity already exists
    Existing { auth_client_id: String },
    /// The identity needs to be provisioned
    Provision(ParentAuthMethod),
}

/// Represents fleet parameters to generate configuration depending of it its enabled or not.
#[derive(Debug)]
pub enum FleetParams {
    FleetDisabled,
    FleetEnabled {
        fleet_id: String,
        identity: SystemIdentitySpec,
    },
}

/// Valid parameters to generate Agent Control configuration, represent [Args] after validation.
#[derive(Debug)]
pub struct Params {
    output_path: PathBuf,
    region: Region,
    proxy_config: Option<ProxyConfig>,
    newrelic_license_key: String,
    env_vars_file_path: Option<PathBuf>,
    fleet: FleetParams,
}

impl Args {
    /// Performs additional args validation (not covered by clap's arguments)
    pub fn validate(self) -> Result<Params, String> {
        let fleet_inputs = if self.fleet_disabled {
            FleetParams::FleetDisabled
        } else {
            // Fleet-id is required
            if self.fleet_id.is_empty() {
                return Err(String::from("'fleet_id' should be set when enabling fleet"));
            }
            let private_key_path = self
                .auth_private_key_path
                .as_ref()
                .ok_or_else(|| {
                    "'auth_private_key_path' needs to be set to register System Identity"
                        .to_string()
                })?
                .clone();

            FleetParams::FleetEnabled {
                fleet_id: self.fleet_id,
                identity: SystemIdentitySpec {
                    system_identity_data: Self::identity_data(
                        &private_key_path,
                        self.auth_client_id,
                        self.identity,
                    )?,
                    private_key_path,
                },
            }
        };
        if let Some(proxy_config) = self.proxy_config.clone()
            && let Err(err) = crate::http::config::ProxyConfig::try_from(proxy_config)
        {
            return Err(format!("invalid proxy configuration: {err}"));
        }
        Ok(Params {
            output_path: self.output_path,
            region: self.region,
            proxy_config: self.proxy_config,
            newrelic_license_key: self.newrelic_license_key,
            env_vars_file_path: self.env_vars_file_path,
            fleet: fleet_inputs,
        })
    }

    /// Helper to build [SystemIdentityData] from args
    fn identity_data(
        private_key_path: &Path,
        auth_client_id: String,
        identity_args: ProvisionIdentityArgs,
    ) -> Result<SystemIdentityData, String> {
        if !auth_client_id.is_empty() {
            if !private_key_path.exists() {
                return Err(
                    "when 'auth_client_id' is provided, 'auth_private_key_path' must exist"
                        .to_string(),
                );
            }
            return Ok(SystemIdentityData::Existing { auth_client_id });
        }
        Ok(SystemIdentityData::Provision(identity_args.validate()?))
    }
}

/// Generates:
/// 1. The Agent Control configuration file according to the provided args.
/// 2. The system identity required for Fleet Control, if applicable.
/// 3. The environment variables file required for the agents, if applicable.
pub fn generate(params: Params) -> Result<(), CliError> {
    write_config_and_generate_system_identity(&params)?;
    write_env_var_config(&params)?;
    Ok(())
}

/// Generates the Agent Control configuration, the system identity and any requisite according to the provided inputs.
fn write_config_and_generate_system_identity(params: &Params) -> Result<(), CliError> {
    info!("Generating Agent Control configuration");

    let yaml = generate_config_and_system_identity(params, create_identity)?;

    LocalFile.write(&params.output_path, yaml).map_err(|err| {
        CliError::Command(format!(
            "error writing the configuration file to '{}': {}",
            params.output_path.to_string_lossy(),
            err
        ))
    })?;
    info!(config_path=%params.output_path.display(), "Agent Control configuration generated successfully");
    Ok(())
}

/// Generates and writes the environment variables configuration file if requested.
fn write_env_var_config(params: &Params) -> Result<(), CliError> {
    let Some(path) = &params.env_vars_file_path else {
        info!("No environment variables file path provided, skipping generation");
        return Ok(());
    };

    info!("Generating environment variables configuration");

    let yaml = generate_env_var_config(params)?;

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
fn generate_env_var_config(params: &Params) -> Result<String, CliError> {
    info!("Inserting OTEL endpoint env var");
    let mut env_vars = HashMap::from([(
        OTLP_ENDPOINT_ENV_VAR.to_string(),
        params.region.otel_endpoint().to_string(),
    )]);

    if !params.newrelic_license_key.is_empty() {
        info!("Inserting New Relic license key env var");
        env_vars.insert(
            NR_LICENSE_ENV_VAR.to_string(),
            params.newrelic_license_key.clone(),
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
    params: &Params,
    create_identity: F,
) -> Result<String, CliError>
where
    F: Fn(&ParentAuthMethod, Region, Option<ProxyConfig>, PublicKeyPem) -> Result<String, CliError>,
{
    let fleet_control = match &params.fleet {
        FleetParams::FleetDisabled => None,
        FleetParams::FleetEnabled {
            fleet_id,
            identity: identity_spec,
        } => {
            let client_id = match &identity_spec.system_identity_data {
                SystemIdentityData::Existing { auth_client_id } => auth_client_id.to_string(),
                SystemIdentityData::Provision(parent_auth_method) => {
                    let pub_key = LocalKeyPairGenerator::from(LocalKeyPairGeneratorConfig {
                        key_type: KeyType::Rsa4096,
                        file_path: identity_spec.private_key_path.clone(),
                    })
                    .generate()
                    .map_err(|err| {
                        CliError::Command(format!(
                            "could not generate System Identity's key-pair; {err}"
                        ))
                    })?;

                    create_identity(
                        parent_auth_method,
                        params.region,
                        params.proxy_config.clone(),
                        pub_key,
                    )?
                }
            };

            Some(FleetControl {
                endpoint: params.region.opamp_endpoint().to_string(),
                signature_validation: SignatureValidation {
                    public_key_server_url: params.region.public_key_endpoint().to_string(),
                },
                fleet_id: fleet_id.to_string(),
                auth_config: AuthConfig {
                    token_url: params.region.token_renewal_endpoint().to_string(),
                    client_id,
                    provider: "local".to_string(),
                    private_key_path: identity_spec.private_key_path.to_string_lossy().to_string(),
                },
            })
        }
    };
    let config = Config {
        fleet_control,
        server: Server { enabled: true },
        proxy: params.proxy_config.clone(),
        agents: HashMap::new(),
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
    use crate::cli::common::system_identity::ParentAuthMethod;
    use assert_matches::assert_matches;
    use clap::{CommandFactory, FromArgMatches};
    use rstest::rstest;
    use std::env::current_dir;
    use tempfile::tempdir;

    impl Default for Params {
        fn default() -> Self {
            Params {
                output_path: Default::default(),
                region: Region::US,
                proxy_config: None,
                newrelic_license_key: Default::default(),
                env_vars_file_path: Default::default(),
                fleet: FleetParams::FleetDisabled,
            }
        }
    }

    #[rstest]
    #[case::fleet_disabled(
        || String::from("--fleet-disabled --output-path /some/path --region us")
    )]
    #[case::token_based_identity(
        || format!("--output-path /some/path --region us --fleet-id some-id --auth-private-key-path {} --auth-parent-token TOKEN --auth-parent-client-id id --organization-id org-id", pwd())
    )]
    #[case::client_id_and_secret_based_identity(
        || format!("--output-path /some/path --region us --fleet-id some-id --auth-private-key-path {} --auth-parent-client-secret SECRET --auth-parent-client-id id --organization-id org-id", pwd())
    )]
    fn test_args_validation(#[case] args: fn() -> String) {
        let cmd = Args::command().no_binary_name(true);
        let matches = cmd
            .try_get_matches_from(args().split_ascii_whitespace())
            .expect("arguments should be valid");
        let args = Args::from_arg_matches(&matches).expect("should create the struct back");
        assert_matches!(args.validate(), Ok(_));
    }

    #[test]
    fn test_identity_already_provided() {
        let args_definition = format!(
            "--output-path /some/path --region us --fleet-id some-id --auth-private-key-path {} --auth-client-id some-client-id",
            pwd()
        );
        let cmd = Args::command().no_binary_name(true);
        let matches = cmd
            .try_get_matches_from(args_definition.split_ascii_whitespace())
            .expect("arguments should be valid");
        let args = Args::from_arg_matches(&matches).expect("should create the struct back");
        assert_matches!(args.validate(), Ok(params) => {
            assert_matches!(params.fleet, FleetParams::FleetEnabled { fleet_id, identity } => {
                assert_eq!(fleet_id, "some-id".to_string());
                assert_matches!(identity.system_identity_data, SystemIdentityData::Existing { auth_client_id } => {
                    assert_eq!(auth_client_id, "some-client-id".to_string());
                })
            })
        })
    }

    #[rstest]
    #[case::missing_identity_creation_method(
        || format!("--output-path /some/path --region us --auth-private-key-path {}", pwd())
    )]
    #[case::missing_private_key_path(
        || String::from("--output-path /some/path --region us --auth-client-id some-client-id")
    )]
    #[case::nonexisting_private_key_path(
        || String::from("--output-path /some/path --region us --auth-client-id some-client-id --auth-private-key-path /do-not/exist")
    )]
    #[case::missing_auth_parent_client_id_with_token(
        || format!("--output-path /some/path --region us --auth-private-key-path {} --auth-parent-token TOKEN --organization-id org-id", pwd())
    )]
    #[case::missing_org_id_with_token(
        || format!("--output-path /some/path --region us --auth-private-key-path {} --auth-parent-token TOKEN --auth-parent-client-id id", pwd())
    )]
    #[case::missing_org_id_with_secret(
        || format!("--output-path /some/path --region us --auth-private-key-path {} --auth-parent-client-secret SECRET --organization-id org-id", pwd())
    )]
    #[case::missing_auth_parent_client_id_with_secret(
        || format!("--output-path /some/path --region us --auth-private-key-path {} --auth-parent-client-secret SECRET --auth-parent-client-id id", pwd())
    )]
    #[case::invalid_proxy_config(
        || String::from("--fleet-disabled --output-path /some/path --region us --proxy-url https::/invalid")
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
    #[case(false, Region::US, None, expected_us())]
    #[case(false, Region::EU, None, expected_eu())]
    #[case(false, Region::STAGING, None, expected_staging())]
    #[case(true, Region::US, None, expected_fleet_disabled())]
    #[case(false, Region::US, some_proxy_config(), expected_us_proxy())]
    fn test_gen_config(
        #[case] fleet_enabled: bool,
        #[case] region: Region,
        #[case] proxy_config: Option<ProxyConfig>,
        #[case] expected: String,
    ) {
        let tmp = tempdir().unwrap();
        let private_key_path = tmp.path().join("private_key");
        let args = create_test_args(
            fleet_enabled,
            region,
            proxy_config,
            private_key_path.clone(),
        );
        // Replacing hardcoded path in expectations because the private-key-path is dynamic
        let expected = expected.replace("/path/to/key", &private_key_path.to_string_lossy());

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
        let args = Params {
            region,
            newrelic_license_key: license.to_string(),
            ..Default::default()
        };

        let yaml = generate_env_var_config(&args).expect("should generate env var config");

        let parsed: HashMap<String, String> =
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

    fn identity_provider_mock(
        _: &ParentAuthMethod,
        _: Region,
        _: Option<ProxyConfig>,
        _: PublicKeyPem,
    ) -> Result<String, CliError> {
        Ok("test-client-id".to_string())
    }

    fn create_test_args(
        fleet_disabled: bool,
        region: Region,
        proxy_config: Option<ProxyConfig>,
        private_key_path: PathBuf,
    ) -> Params {
        let fleet = if fleet_disabled {
            FleetParams::FleetDisabled
        } else {
            FleetParams::FleetEnabled {
                fleet_id: "test-fleet-id".to_string(),
                identity: SystemIdentitySpec {
                    system_identity_data: SystemIdentityData::Provision(
                        ParentAuthMethod::ParentSecret {
                            secret: "parent-client-secret".to_string(),
                            parent_client_id: "parent-client-id".to_string(),
                            organization_id: "test-org-id".to_string(),
                        },
                    ),
                    private_key_path,
                },
            }
        };
        Params {
            output_path: PathBuf::from("/tmp/config.yaml"),
            region,
            proxy_config,
            newrelic_license_key: "test-license-key".to_string(),
            env_vars_file_path: None,
            fleet,
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
    private_key_path: /path/to/key

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
    private_key_path: /path/to/key

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
    private_key_path: /path/to/key

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

    fn expected_us() -> String {
        format!(
            "{}{}{}{}",
            FLEET_CONTROL_US,
            SERVER_SECTION,
            NO_AGENTS_SECTION,
            log_section()
        )
    }

    fn expected_eu() -> String {
        format!(
            "{}{}{}{}",
            FLEET_CONTROL_EU,
            SERVER_SECTION,
            NO_AGENTS_SECTION,
            log_section()
        )
    }

    fn expected_staging() -> String {
        format!(
            "{}{}{}{}",
            FLEET_CONTROL_STAGING,
            SERVER_SECTION,
            NO_AGENTS_SECTION,
            log_section()
        )
    }

    fn expected_fleet_disabled() -> String {
        format!("{}{}{}", SERVER_SECTION, NO_AGENTS_SECTION, log_section())
    }

    fn expected_us_proxy() -> String {
        format!(
            "{}{}{}{}{}",
            FLEET_CONTROL_US,
            SERVER_SECTION,
            PROXY_SECTION,
            NO_AGENTS_SECTION,
            log_section()
        )
    }
}
