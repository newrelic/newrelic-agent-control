//! This module provides the functions to handle identity creation when setting up Agent Control.
//!
use std::{path::PathBuf, time::Duration};

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
#[derive(Debug, Default, clap::Args)]
pub struct IdentityArgs {
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

impl IdentityArgs {
    /// Performs additional args validation (not covered by clap's arguments)
    pub fn validate(&self) -> Result<(), String> {
        // Any method to provide the identity should be selected
        if self.auth_client_id.is_empty()
            && self.auth_parent_token.is_empty()
            && self.auth_parent_client_secret.is_empty()
        {
            return Err(String::from(
                "either 'auth_client_id', 'auth_parent_token' or 'auth_parent_secret' should be set to register System Identity",
            ));
        }
        // 'auth_private_key_path' is required
        let Some(auth_private_key_path) = self.auth_private_key_path.as_ref() else {
            return Err(String::from(
                "'auth_private_key_path' needs to be set to register System Identity",
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

        Ok(())
    }
}

/// Provides a key-based identity considering the supplied args.
pub fn provide_identity(
    identity_args: &IdentityArgs,
    region: Region,
    proxy_config: Option<ProxyConfig>,
) -> Result<Identity, CliError> {
    let environment = NewRelicEnvironment::from(region);
    build_identity(identity_args, environment, proxy_config)
}

/// Helper to allow injecting testing urls when building the identity.
fn build_identity(
    identity_args: &IdentityArgs,
    environment: NewRelicEnvironment,
    proxy_config: Option<ProxyConfig>,
) -> Result<Identity, CliError> {
    let auth_private_key_path = identity_args.auth_private_key_path.clone().ok_or_else(|| {
        CliError::Command("'auth-private-key-path' is required when fleet is enabled".to_string())
    })?;

    // Already provided identity
    if !identity_args.auth_client_id.is_empty() {
        info!("Using provided System Identity");
        return Ok(Identity {
            client_id: identity_args.auth_client_id.to_string(),
            private_key_path: auth_private_key_path,
        });
    }

    info!("Generating System Identity");
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

    let token = if identity_args.auth_parent_token.is_empty() {
        info!("Retrieving token using the provided client-id + secret");
        // Generate the parent token if it wasn't provided
        let authenticator =
            HttpAuthenticator::new(http_client.clone(), environment.token_renewal_endpoint());

        let token_retriever = TokenRetrieverWithCache::new_with_secret(
            identity_args.auth_parent_client_id.clone(),
            authenticator,
            identity_args.auth_parent_client_secret.clone().into(),
        )
        .with_retries(DEFAULT_RETRIES);

        token_retriever.retrieve().map_err(|err| {
            CliError::Command(format!(
                "could not retrieve the token to create the system identity: {err}"
            ))
        })?
    } else {
        info!("Using the provided token for authentication");
        Token::new(
            identity_args.auth_parent_token.clone(),
            TokenType::Bearer,
            Default::default(),
        )
    };

    let output_platform = OutputPlatform::LocalPrivateKeyPath(auth_private_key_path.clone());

    let system_identity_creation_metadata = SystemIdentityCreationMetadata {
        system_identity_input: SystemIdentityInput {
            organization_id: identity_args.organization_id.clone(),
            client_id: identity_args.auth_parent_client_id.clone(),
        },
        name: None,
        environment,
        output_platform,
    };
    let iam_client = HttpIAMClient::new(http_client, system_identity_creation_metadata.to_owned());

    let key_creator = LocalCreator::from(KeyPairGeneratorLocalConfig {
        key_type: KeyType::Rsa4096,
        file_path: auth_private_key_path.clone(),
    });

    let system_identity_generator = L2SystemIdentityGenerator {
        iam_client,
        key_creator,
    };

    let result = system_identity_generator
        .generate(&token)
        .map_err(|err| CliError::Command(format!("error generating the system identity: {err}")))?;

    info!(
        private_key_path = %auth_private_key_path.to_string_lossy(),
        "System Identity successfully generated"
    );

    Ok(Identity {
        client_id: result.client_id,
        private_key_path: auth_private_key_path,
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
    use http::header::AUTHORIZATION;
    use httpmock::{Method::POST, MockServer};
    use std::fs;
    use tempfile::TempDir;

    use super::*;

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
        let identity_args = IdentityArgs {
            auth_private_key_path: Some(auth_private_key_path.clone()),
            auth_client_id: "provided_client_id".to_string(),
            ..Default::default()
        };

        let identity =
            build_identity(&identity_args, environment, None).expect("no error expected");
        assert_eq!(identity.client_id, "provided_client_id".to_string());
        assert_eq!(identity.private_key_path, auth_private_key_path);
    }

    #[test]
    fn test_build_identity_with_token() {
        let tempdir = TempDir::new().unwrap();
        let auth_private_key_path = tempdir.path().join("private-key");

        let server = MockServer::start();

        let identity_args = IdentityArgs {
            auth_parent_client_id: "parent-client-id".to_string(),
            auth_parent_token: "TOKEN".to_string(),
            auth_private_key_path: Some(auth_private_key_path.clone()),
            ..Default::default()
        };

        // Expect a request to create the identity
        let identity_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/identity")
                .header_includes(AUTHORIZATION.as_str(), "Bearer TOKEN");
            then.status(200).body(identity_body(
                "created_client_id",
                &identity_args.organization_id,
            ));
        });

        let environment = NewRelicEnvironment::Custom {
            token_renewal_endpoint: "https://should-not-call.this"
                .parse()
                .expect("url should be valid"),
            system_identity_creation_uri: format!("{}/identity", server.base_url())
                .parse()
                .expect("url should be valid"),
        };

        let identity =
            build_identity(&identity_args, environment, None).expect("no error expected");

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

        let identity_args = IdentityArgs {
            auth_parent_client_id: "parent-client-id".to_string(),
            auth_parent_client_secret: "client-secret-value".to_string(),
            auth_private_key_path: Some(auth_private_key_path.clone()),
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
            then.status(200).body(identity_body(
                "created_client_id",
                &identity_args.organization_id,
            ));
        });

        let environment = NewRelicEnvironment::Custom {
            token_renewal_endpoint: format!("{}/token", server.base_url())
                .parse()
                .expect("url should be valid"),
            system_identity_creation_uri: format!("{}/identity", server.base_url())
                .parse()
                .expect("url should be valid"),
        };

        let identity =
            build_identity(&identity_args, environment, None).expect("no error expected");

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
