//! This module provides the functions to handle identity creation when setting up Agent Control.
//!
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use nr_auth::{
    TokenRetriever,
    authenticator::HttpAuthenticator,
    http::{client::HttpClient, config::HttpConfig},
    key::{
        creator::KeyType,
        local::{KeyPairGeneratorLocalConfig, LocalCreator},
    },
    system_identity::{
        generator::L2SystemIdentityGenerator,
        iam_client::http::HttpIAMClient,
        input_data::{
            SystemIdentityCreationMetadata, SystemIdentityInput, environment::NewRelicEnvironment,
            output_platform::OutputPlatform,
        },
    },
    token::{Token, TokenType},
    token_retriever::TokenRetrieverWithCache,
};
use tracing::info;

use crate::cli::common::{error::CliError, proxy_config::ProxyConfig, region::Region};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);
const DEFAULT_RETRIES: u8 = 3;

/// Represents a key-based identity to be used in Agent Control configuration.
pub struct Identity {
    pub client_id: String,
    pub private_key_path: PathBuf,
}

/// Arguments required to provide or generate a system identity.
///
/// **Design note:** modelling the different provisioning methods as subcommands would be
/// a more natural fit for clap (each subcommand carrying only its own required fields),
/// but it was deliberately avoided. These arguments are shared across several external
/// installation tools — the Linux recipe, the Windows installation script, and the
/// Kubernetes installation job — none of which can easily branch on a subcommand name.
/// A flat, optional-field struct means the logic for selecting the provisioning method
/// (reuse an existing identity, generate a new one, obtain a bearer token via client
/// credentials, or accept a pre-obtained token) lives here rather than being duplicated
/// in each installer.
#[derive(Debug, Default, clap::Args)]
pub struct SystemIdentityArgs {
    /// Path where the private key is stored or will be written.
    #[arg(long)]
    pub auth_private_key_path: Option<PathBuf>,

    /// Client ID of an already-provisioned system identity. When non-empty, no identity
    /// generation is performed.
    #[arg(long, default_value_t)]
    pub auth_client_id: String,

    /// Client ID of the parent system identity, used to obtain a token or as metadata
    /// during identity generation.
    #[arg(long, default_value_t)]
    pub auth_parent_client_id: String,

    /// Client secret of the parent system identity, used to obtain a bearer token.
    #[arg(long, default_value_t)]
    pub auth_parent_client_secret: String,

    /// Pre-obtained bearer token for the parent system identity. When non-empty, token
    /// retrieval via client-id + secret is skipped.
    #[arg(long, default_value_t)]
    pub auth_parent_token: String,

    /// Organization ID associated with the new system identity.
    #[arg(long, default_value_t)]
    pub organization_id: String,
}

#[derive(Debug)]
pub enum ProvisioningMethod {
    ExistingIdentity {
        auth_client_id: String,
    },
    ParentToken {
        token: String,
        parent_client_id: String,
        organization_id: String,
    },
    ParentSecret {
        secret: String,
        parent_client_id: String,
        organization_id: String,
    },
}

/// Valid data to create a SystemIdentity, represent [SystemIdentityArgs] after validation.
#[derive(Debug)]
pub struct SystemIdentitySpec {
    pub method: ProvisioningMethod,
    pub private_key_path: PathBuf,
}

impl SystemIdentityArgs {
    /// Performs additional args validation (not covered by clap's arguments) and returns [SystemIdentityData] if
    /// validation was Ok.
    pub fn validate(self) -> Result<SystemIdentitySpec, String> {
        let private_key_path = self
            .auth_private_key_path
            .as_ref()
            .ok_or_else(|| {
                "'auth_private_key_path' needs to be set to register System Identity".to_string()
            })?
            .clone();

        let method = self.resolve_provisioning_method(&private_key_path)?;

        Ok(SystemIdentitySpec {
            method,
            private_key_path,
        })
    }

    fn resolve_provisioning_method(self, key_path: &Path) -> Result<ProvisioningMethod, String> {
        if !self.auth_client_id.is_empty() {
            if !key_path.exists() {
                return Err(
                    "when 'auth_client_id' is provided the 'auth_private_key_path' must also be provided and exist"
                        .to_string(),
                );
            }
            return Ok(ProvisioningMethod::ExistingIdentity {
                auth_client_id: self.auth_client_id,
            });
        }

        if !self.auth_parent_token.is_empty() {
            self.require_org_and_parent_id("token based")?;
            return Ok(ProvisioningMethod::ParentToken {
                token: self.auth_parent_token,
                parent_client_id: self.auth_parent_client_id,
                organization_id: self.organization_id,
            });
        }

        if !self.auth_parent_client_secret.is_empty() {
            self.require_org_and_parent_id("client-secret based")?;
            return Ok(ProvisioningMethod::ParentSecret {
                secret: self.auth_parent_client_secret,
                parent_client_id: self.auth_parent_client_id,
                organization_id: self.organization_id,
            });
        }

        Err(
            "either 'auth_client_id', 'auth_parent_token' or 'auth_parent_secret' should be set to register System Identity"
                .to_string(),
        )
    }

    fn require_org_and_parent_id(&self, mode: &str) -> Result<(), String> {
        if self.organization_id.is_empty() || self.auth_parent_client_id.is_empty() {
            return Err(format!(
                "{mode} system identity generation requires 'auth_parent_client_id' and 'organization_id'"
            ));
        }
        Ok(())
    }
}

/// Provides a key-based identity considering the supplied args.
pub fn provide_identity(
    identity_input: &SystemIdentitySpec,
    region: Region,
    proxy_config: Option<ProxyConfig>,
) -> Result<Identity, CliError> {
    let environment = NewRelicEnvironment::from(region);
    build_identity(identity_input, environment, proxy_config)
}

/// Helper to allow injecting testing urls when building the identity.
fn build_identity(
    identity_input: &SystemIdentitySpec,
    environment: NewRelicEnvironment,
    proxy_config: Option<ProxyConfig>,
) -> Result<Identity, CliError> {
    let SystemIdentitySpec {
        private_key_path,
        method,
    } = identity_input;

    match method {
        ProvisioningMethod::ExistingIdentity { auth_client_id } => {
            info!("Using provided System Identity");
            Ok(Identity {
                client_id: auth_client_id.to_string(),
                private_key_path: private_key_path.clone(),
            })
        }
        ProvisioningMethod::ParentToken {
            token,
            parent_client_id,
            organization_id,
        } => {
            let token = Token::new(token.to_string(), TokenType::Bearer, Default::default());
            let http_client = http_client(proxy_config)?;
            build_identity_from_token(
                token,
                private_key_path.clone(),
                organization_id.clone(),
                parent_client_id.clone(),
                environment,
                http_client,
            )
        }
        ProvisioningMethod::ParentSecret {
            secret,
            parent_client_id,
            organization_id,
        } => {
            let http_client = http_client(proxy_config)?;
            let token = get_auth_token(
                parent_client_id.clone(),
                secret.clone(),
                &environment,
                http_client.clone(),
            )?;
            build_identity_from_token(
                token,
                private_key_path.clone(),
                organization_id.clone(),
                parent_client_id.clone(),
                environment,
                http_client,
            )
        }
    }
}

fn http_client(proxy_config: Option<ProxyConfig>) -> Result<HttpClient, CliError> {
    let nr_auth_proxy_config = proxy_config
        .map(build_nr_auth_proxy_config)
        .transpose()?
        .unwrap_or_default();

    let http_config = HttpConfig::new(DEFAULT_TIMEOUT, DEFAULT_TIMEOUT, nr_auth_proxy_config);
    let http_client = HttpClient::new(http_config).map_err(|err| {
        CliError::Command(format!(
            "client error setting up the system identity: {err}"
        ))
    })?;

    Ok(http_client)
}

fn get_auth_token(
    parent_client_id: String,
    secret: String,
    environment: &NewRelicEnvironment,
    http_client: HttpClient,
) -> Result<Token, CliError> {
    info!("Retrieving token using the provided client-id + secret");
    let authenticator =
        HttpAuthenticator::new(http_client.clone(), environment.token_renewal_endpoint());

    let token_retriever = TokenRetrieverWithCache::new_with_secret(
        parent_client_id.clone(),
        authenticator,
        secret.into(),
    )
    .with_retries(DEFAULT_RETRIES);

    token_retriever.retrieve().map_err(|err| {
        CliError::Command(format!(
            "could not retrieve the token to create the system identity: {err}"
        ))
    })
}

fn build_identity_from_token(
    token: Token,
    private_key_path: PathBuf,
    organization_id: String,
    parent_client_id: String,
    environment: NewRelicEnvironment,
    http_client: HttpClient,
) -> Result<Identity, CliError> {
    let output_platform = OutputPlatform::LocalPrivateKeyPath(private_key_path.clone());

    let system_identity_creation_metadata = SystemIdentityCreationMetadata {
        system_identity_input: SystemIdentityInput {
            organization_id: organization_id.clone(),
            client_id: parent_client_id.clone(),
        },
        name: None,
        environment,
        output_platform,
    };
    let iam_client = HttpIAMClient::new(http_client, system_identity_creation_metadata.to_owned());

    let key_creator = LocalCreator::from(KeyPairGeneratorLocalConfig {
        key_type: KeyType::Rsa4096,
        file_path: private_key_path.clone(),
    });

    let system_identity_generator = L2SystemIdentityGenerator {
        iam_client,
        key_creator,
    };

    let result = system_identity_generator
        .generate(&token)
        .map_err(|err| CliError::Command(format!("error generating the system identity: {err}")))?;

    info!(
        private_key_path = %private_key_path.to_string_lossy(),
        "System Identity successfully generated"
    );

    Ok(Identity {
        client_id: result.client_id,
        private_key_path,
    })
}

/// Builds the proxy config for nr-auth from the AC's system proxy
fn build_nr_auth_proxy_config(
    ac_cfg: ProxyConfig,
) -> Result<nr_auth::http::config::ProxyConfig, CliError> {
    let auth_cfg = nr_auth::http::config::ProxyConfig::new(
        ac_cfg.proxy_url.unwrap_or_default(),
        PathBuf::from(ac_cfg.proxy_ca_bundle_dir.unwrap_or_default()),
        PathBuf::from(ac_cfg.proxy_ca_bundle_file.unwrap_or_default()),
    )
    .map_err(|err| CliError::Command(format!("invalid proxy configuration: {err}")))?;

    if ac_cfg.ignore_system_proxy {
        Ok(auth_cfg)
    } else {
        auth_cfg
            .try_with_url_from_env()
            .map_err(|err| CliError::Command(format!("invalid proxy configuration: {err}")))
    }
}

#[cfg(test)]
pub mod tests {
    use assert_matches::assert_matches;
    use http::header::AUTHORIZATION;
    use httpmock::{Method::POST, MockServer};
    use rstest::rstest;
    use std::fs;
    use tempfile::TempDir;

    use super::*;

    #[rstest]
    #[case::existing_identity(|| SystemIdentityArgs {
        auth_private_key_path: Some(std::env::current_dir().unwrap()),
        auth_client_id: "some-client-id".to_string(),
        ..Default::default()
    })]
    #[case::parent_token(|| SystemIdentityArgs {
        auth_private_key_path: Some(PathBuf::from("/some/path")),
        auth_parent_token: "TOKEN".to_string(),
        auth_parent_client_id: "parent-id".to_string(),
        organization_id: "org-id".to_string(),
        ..Default::default()
    })]
    #[case::parent_secret(|| SystemIdentityArgs {
        auth_private_key_path: Some(PathBuf::from("/some/path")),
        auth_parent_client_secret: "SECRET".to_string(),
        auth_parent_client_id: "parent-id".to_string(),
        organization_id: "org-id".to_string(),
        ..Default::default()
    })]
    fn test_validate(#[case] make_args: fn() -> SystemIdentityArgs) {
        assert_matches!(make_args().validate(), Ok(_));
    }

    #[rstest]
    #[case::missing_private_key_path(|| SystemIdentityArgs {
        auth_client_id: "some-client-id".to_string(),
        ..Default::default()
    })]
    #[case::nonexistent_key_with_client_id(|| SystemIdentityArgs {
        auth_private_key_path: Some(PathBuf::from("/does/not/exist")),
        auth_client_id: "some-client-id".to_string(),
        ..Default::default()
    })]
    #[case::token_missing_org_id(|| SystemIdentityArgs {
        auth_private_key_path: Some(PathBuf::from("/some/path")),
        auth_parent_token: "TOKEN".to_string(),
        auth_parent_client_id: "parent-id".to_string(),
        ..Default::default()
    })]
    #[case::token_missing_parent_client_id(|| SystemIdentityArgs {
        auth_private_key_path: Some(PathBuf::from("/some/path")),
        auth_parent_token: "TOKEN".to_string(),
        organization_id: "org-id".to_string(),
        ..Default::default()
    })]
    #[case::secret_missing_org_id(|| SystemIdentityArgs {
        auth_private_key_path: Some(PathBuf::from("/some/path")),
        auth_parent_client_secret: "SECRET".to_string(),
        auth_parent_client_id: "parent-id".to_string(),
        ..Default::default()
    })]
    #[case::secret_missing_parent_client_id(|| SystemIdentityArgs {
        auth_private_key_path: Some(PathBuf::from("/some/path")),
        auth_parent_client_secret: "SECRET".to_string(),
        organization_id: "org-id".to_string(),
        ..Default::default()
    })]
    #[case::no_method_provided(|| SystemIdentityArgs {
        auth_private_key_path: Some(PathBuf::from("/some/path")),
        ..Default::default()
    })]
    fn test_validate_errors(#[case] make_args: fn() -> SystemIdentityArgs) {
        assert_matches!(make_args().validate(), Err(_));
    }

    #[test]
    fn test_build_identity_already_provided() {
        let tempdir = TempDir::new().unwrap();
        let auth_private_key_path = tempdir.path().join("private-key");
        // Expect no request because the identity was already provided
        let environment = NewRelicEnvironment::Custom {
            token_renewal_endpoint: "https://should-not-call.this"
                .parse()
                .expect("url should be valid"),
            system_identity_creation_uri: "https://should-not-call.this"
                .parse()
                .expect("url should be valid"),
        };
        // Key file must exist when using ExistingIdentity
        fs::write(&auth_private_key_path, "").unwrap();
        let identity_args = SystemIdentityArgs {
            auth_private_key_path: Some(auth_private_key_path.clone()),
            auth_client_id: "provided_client_id".to_string(),
            ..Default::default()
        };
        let identity_data = identity_args.validate().expect("validation should succeed");

        let identity =
            build_identity(&identity_data, environment, None).expect("no error expected");
        assert_eq!(identity.client_id, "provided_client_id".to_string());
        assert_eq!(identity.private_key_path, auth_private_key_path);
    }

    #[test]
    fn test_build_identity_with_token() {
        let tempdir = TempDir::new().unwrap();
        let auth_private_key_path = tempdir.path().join("private-key");

        let server = MockServer::start();

        let identity_args = SystemIdentityArgs {
            auth_parent_client_id: "parent-client-id".to_string(),
            auth_parent_token: "TOKEN".to_string(),
            auth_private_key_path: Some(auth_private_key_path.clone()),
            organization_id: "test-org-id".to_string(),
            ..Default::default()
        };

        // Expect a request to create the identity
        let identity_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/identity")
                .header_includes(AUTHORIZATION.as_str(), "Bearer TOKEN");
            then.status(200)
                .body(identity_body("created_client_id", "test-org-id"));
        });

        let environment = NewRelicEnvironment::Custom {
            token_renewal_endpoint: "https://should-not-call.this"
                .parse()
                .expect("url should be valid"),
            system_identity_creation_uri: format!("{}/identity", server.base_url())
                .parse()
                .expect("url should be valid"),
        };

        let identity_data = identity_args.validate().expect("validation should succeed");
        let identity =
            build_identity(&identity_data, environment, None).expect("no error expected");

        identity_mock.assert_calls(1);
        assert_eq!(identity.client_id, "created_client_id".to_string());
        assert_eq!(identity.private_key_path, auth_private_key_path);
        assert!(
            fs::read_to_string(&auth_private_key_path)
                .unwrap()
                .contains("BEGIN PRIVATE KEY"),
        );
    }

    #[test]
    fn test_build_identity_client_secret() {
        let tempdir = TempDir::new().unwrap();
        let auth_private_key_path = tempdir.path().join("private-key");

        let server = MockServer::start();

        let identity_args = SystemIdentityArgs {
            auth_parent_client_id: "parent-client-id".to_string(),
            auth_parent_client_secret: "client-secret-value".to_string(),
            auth_private_key_path: Some(auth_private_key_path.clone()),
            organization_id: "test-org-id".to_string(),
            ..Default::default()
        };

        // Expect a request to authenticate (obtain the token) and another to create the identity
        let token_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/token")
                .body_includes("client-secret-value")
                .body_includes("parent-client-id");
            then.status(200).body(token_body("TOKEN-VALUE"));
        });

        let identity_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/identity")
                .header_includes(AUTHORIZATION.as_str(), "Bearer TOKEN-VALUE");
            then.status(200)
                .body(identity_body("created_client_id", "test-org-id"));
        });

        let environment = NewRelicEnvironment::Custom {
            token_renewal_endpoint: format!("{}/token", server.base_url())
                .parse()
                .expect("url should be valid"),
            system_identity_creation_uri: format!("{}/identity", server.base_url())
                .parse()
                .expect("url should be valid"),
        };

        let identity_data = identity_args.validate().expect("validation should succeed");
        let identity =
            build_identity(&identity_data, environment, None).expect("no error expected");

        identity_mock.assert_calls(1);
        token_mock.assert_calls(1);
        assert_eq!(identity.client_id, "created_client_id".to_string());
        assert_eq!(identity.private_key_path, auth_private_key_path);
        assert!(
            fs::read_to_string(&auth_private_key_path)
                .unwrap()
                .contains("BEGIN PRIVATE KEY"),
        );
    }

    fn token_body(token: &str) -> String {
        format!(
            r#"
        {{
          "access_token": "{token}",
          "expires_in": 3600,
          "token_type": "bearer"
        }}
        "#
        )
    }

    fn identity_body(client_id: &str, organization_id: &str) -> String {
        format!(
            r#"{{
            "data": {{
              "systemIdentityCreate": {{
                  "id": "identity-identifier",
                  "clientId": "{client_id}",
                  "organizationId": "{organization_id}",
                  "publicKey": "dmFsdWUK"
                  }}
                }}
        }}"#
        )
    }
}
