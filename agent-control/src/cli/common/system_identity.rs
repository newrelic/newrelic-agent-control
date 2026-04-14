//! This module provides the functions to handle identity creation when setting up Agent Control.
//!
use std::{path::PathBuf, time::Duration};

use nr_auth::{
    TokenRetriever,
    authenticator::HttpAuthenticator,
    http::{client::HttpClient, config::HttpConfig},
    key::generation::PublicKeyPem,
    system_identity::{
        iam_client::http::{HttpIAMClient, IAMAuthCredential},
        identity_creator::L2IdentityCreator,
        input_data::{SystemIdentityCreationMetadata, environment::NewRelicEnvironment},
    },
    token::{Token, TokenType},
    token_retriever::TokenRetrieverWithCache,
};
use tracing::info;

use crate::cli::common::{error::CliError, proxy_config::ProxyConfig, region::Region};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);
const DEFAULT_RETRIES: u8 = 3;

/// Arguments required to provision system identity.
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
pub struct ProvisionIdentityArgs {
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

/// Defines the supported provisioning methods for System Identities
#[derive(Debug)]
pub enum ParentAuthMethod {
    ParentToken {
        token: String,
        organization_id: String,
    },
    ParentSecret {
        secret: String,
        parent_client_id: String,
        organization_id: String,
    },
}

impl ProvisionIdentityArgs {
    pub fn validate(self) -> Result<ParentAuthMethod, String> {
        if !self.auth_parent_token.is_empty() {
            self.require_org_and_parent_id("token based")?;
            return Ok(ParentAuthMethod::ParentToken {
                token: self.auth_parent_token,
                organization_id: self.organization_id,
            });
        }

        if !self.auth_parent_client_secret.is_empty() {
            self.require_org_and_parent_id("client-secret based")?;
            return Ok(ParentAuthMethod::ParentSecret {
                secret: self.auth_parent_client_secret,
                parent_client_id: self.auth_parent_client_id,
                organization_id: self.organization_id,
            });
        }
        Err(
            "either 'auth_parent_token' or 'auth_parent_secret' should be set to create a System Identity"
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

/// Creates a key-based identity considering the supplied args. It returns the corresponding **client_id** as a String.
pub fn create_identity(
    parent_auth_method: &ParentAuthMethod,
    region: Region,
    proxy_config: Option<ProxyConfig>,
    pub_key: PublicKeyPem,
) -> Result<String, CliError> {
    let environment = NewRelicEnvironment::from(region);
    build_identity(parent_auth_method, environment, proxy_config, pub_key)
}

/// Helper to allow injecting testing urls when building the identity.
fn build_identity(
    parent_auth_method: &ParentAuthMethod,
    environment: NewRelicEnvironment,
    proxy_config: Option<ProxyConfig>,
    pub_key: PublicKeyPem,
) -> Result<String, CliError> {
    match parent_auth_method {
        ParentAuthMethod::ParentToken {
            token,
            organization_id,
        } => {
            let token = Token::new(token.to_string(), TokenType::Bearer, Default::default());
            let http_client = http_client(proxy_config)?;
            build_identity_from_token(
                token,
                organization_id.clone(),
                environment,
                http_client,
                pub_key,
            )
        }
        ParentAuthMethod::ParentSecret {
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
                organization_id.clone(),
                environment,
                http_client,
                pub_key,
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

    let token_retriever =
        TokenRetrieverWithCache::new_with_secret(parent_client_id, authenticator, secret.into())
            .with_retries(DEFAULT_RETRIES);

    token_retriever.retrieve().map_err(|err| {
        CliError::Command(format!(
            "could not retrieve the token to create the system identity: {err}"
        ))
    })
}

fn build_identity_from_token(
    token: Token,
    organization_id: String,
    environment: NewRelicEnvironment,
    http_client: HttpClient,
    pub_key: PublicKeyPem,
) -> Result<String, CliError> {
    let system_identity_creation_metadata = SystemIdentityCreationMetadata {
        organization_id: organization_id.clone(),
        name: None,
        environment,
    };
    let iam_client = HttpIAMClient::new(http_client, system_identity_creation_metadata.to_owned());

    let auth_credentials = IAMAuthCredential::BearerToken(token.access_token().to_string());

    let result = iam_client
        .create_l2_system_identity(&auth_credentials, &pub_key)
        .map_err(|err| CliError::Command(format!("error generating the system identity: {err}")))?;

    Ok(result.client_id)
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

    use super::*;

    #[rstest]
    #[case::parent_token(ProvisionIdentityArgs {
        auth_parent_token: "TOKEN".to_string(),
        auth_parent_client_id: "parent-id".to_string(),
        organization_id: "org-id".to_string(),
        ..Default::default()
    })]
    #[case::parent_secret(ProvisionIdentityArgs {
        auth_parent_client_secret: "SECRET".to_string(),
        auth_parent_client_id: "parent-id".to_string(),
        organization_id: "org-id".to_string(),
        ..Default::default()
    })]
    fn test_validate(#[case] make_args: ProvisionIdentityArgs) {
        assert_matches!(make_args.validate(), Ok(_));
    }

    #[rstest]
    #[case::token_missing_org_id(|| ProvisionIdentityArgs {
        auth_parent_token: "TOKEN".to_string(),
        auth_parent_client_id: "parent-id".to_string(),
        ..Default::default()
    })]
    #[case::token_missing_parent_client_id(|| ProvisionIdentityArgs {
        auth_parent_token: "TOKEN".to_string(),
        organization_id: "org-id".to_string(),
        ..Default::default()
    })]
    #[case::secret_missing_org_id(|| ProvisionIdentityArgs {
        auth_parent_client_secret: "SECRET".to_string(),
        auth_parent_client_id: "parent-id".to_string(),
        ..Default::default()
    })]
    #[case::secret_missing_parent_client_id(|| ProvisionIdentityArgs {
        auth_parent_client_secret: "SECRET".to_string(),
        organization_id: "org-id".to_string(),
        ..Default::default()
    })]
    #[case::no_method_provided(|| ProvisionIdentityArgs::default())]
    fn test_validate_errors(#[case] make_args: fn() -> ProvisionIdentityArgs) {
        assert_matches!(make_args().validate(), Err(_));
    }

    #[test]
    fn test_build_identity_with_token() {
        let server = MockServer::start();

        let identity_args = ProvisionIdentityArgs {
            auth_parent_client_id: "parent-client-id".to_string(),
            auth_parent_token: "TOKEN".to_string(),
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

        let method = identity_args.validate().expect("validation should succeed");
        let pub_key = b"mock-pub-key";

        let client_id = build_identity(&method, environment, None, pub_key.to_vec())
            .expect("no error expected");
        assert_eq!(client_id, "created_client_id".to_string());

        identity_mock.assert_calls(1);
    }

    #[test]
    fn test_build_identity_client_secret() {
        let server = MockServer::start();

        let identity_args = ProvisionIdentityArgs {
            auth_parent_client_id: "parent-client-id".to_string(),
            auth_parent_client_secret: "client-secret-value".to_string(),
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

        let method = identity_args.validate().expect("validation should succeed");
        let pub_key = b"mock-public-key";
        let client_id = build_identity(&method, environment, None, pub_key.to_vec())
            .expect("no error expected");
        assert_eq!(client_id, "created_client_id".to_string());

        identity_mock.assert_calls(1);
        token_mock.assert_calls(1);
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
