//! This module provides the functions to handle identity creation when generating Agent Control configuration

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

use crate::{cli::error::CliError, http::config::ProxyConfig};

use super::Args;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);
const DEFAULT_RETRIES: u8 = 3;

/// Represents a key-based identity to be used in Agent Control configuration.
pub struct Identity {
    pub client_id: String,
    pub private_key_path: PathBuf,
}

/// Provides a key-based identity considering the supplied args.
pub fn provide_identity(args: &Args) -> Result<Identity, CliError> {
    let environment = NewRelicEnvironment::from(args.region);
    build_identity(args, environment)
}

/// Helper to allow injecting testing urls when building the identity.
fn build_identity(args: &Args, environment: NewRelicEnvironment) -> Result<Identity, CliError> {
    // Already provided identity
    if !args.auth_client_id.is_empty() {
        return Ok(Identity {
            client_id: args.auth_client_id.to_string(),
            private_key_path: args.auth_private_key_path.clone(),
        });
    }

    info!("Generating System Identity");
    let proxy_config = args
        .proxy_config
        .clone()
        .map(build_nr_auth_proxy_config)
        .transpose()?
        .unwrap_or_default();

    let http_config = HttpConfig::new(DEFAULT_TIMEOUT, DEFAULT_TIMEOUT, proxy_config);
    let http_client = HttpClient::new(http_config).map_err(|err| {
        CliError::Command(format!(
            "client error setting up the system identity: {err}"
        ))
    })?;

    let token = if args.auth_parent_token.is_empty() {
        // Generate the parent token if it wasn't provided
        let authenticator =
            HttpAuthenticator::new(http_client.clone(), environment.token_renewal_endpoint());

        let token_retriever = TokenRetrieverWithCache::new_with_secret(
            args.auth_parent_client_id.clone(),
            authenticator,
            args.auth_parent_client_secret.clone().into(),
        )
        .with_retries(DEFAULT_RETRIES);

        token_retriever.retrieve().map_err(|err| {
            CliError::Command(format!(
                "could not retrieve the token to create the system identity: {err}"
            ))
        })?
    } else {
        Token::new(
            args.auth_parent_token.clone(),
            TokenType::Bearer,
            Default::default(),
        )
    };

    let output_platform =
        OutputPlatform::LocalPrivateKeyPath(args.auth_private_key_path.to_owned());

    let system_identity_creation_metadata = SystemIdentityCreationMetadata {
        system_identity_input: SystemIdentityInput {
            organization_id: args.organization_id.clone(),
            client_id: args.auth_parent_client_id.clone(),
        },
        name: None,
        environment,
        output_platform,
    };
    let iam_client = HttpIAMClient::new(http_client, system_identity_creation_metadata.to_owned());

    let key_creator = LocalCreator::from(KeyPairGeneratorLocalConfig {
        key_type: KeyType::Rsa4096,
        file_path: args.auth_private_key_path.clone(),
    });

    let system_identity_generator = L2SystemIdentityGenerator {
        iam_client,
        key_creator,
    };

    let result = system_identity_generator
        .generate(&token)
        .map_err(|err| CliError::Command(format!("error generating the system identity: {err}")))?;

    info!(
        private_key_path = %args.auth_private_key_path.to_string_lossy(),
        "System Identity successfully generated"
    );

    Ok(Identity {
        client_id: result.client_id,
        private_key_path: args.auth_private_key_path.clone(),
    })
}

/// Builds the proxy config for nr-auth from the AC's system proxy
fn build_nr_auth_proxy_config(
    ac_cfg: ProxyConfig,
) -> Result<nr_auth::http::config::ProxyConfig, CliError> {
    let auth_cfg = nr_auth::http::config::ProxyConfig::new(
        ac_cfg.url_as_string(),
        ac_cfg.ca_bundle_dir().to_owned(),
        ac_cfg.ca_bundle_file().to_owned(),
    )
    .map_err(|err| CliError::Command(format!("invalid proxy configuration: {err}")))?;

    if ac_cfg.ignore_system_proxy() {
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
    use tempfile::TempDir;

    use crate::cli::on_host::config_gen::{config::AgentSet, region::Region};
    use std::fs;

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
        let args = testing_args(
            "",
            "",
            "",
            auth_private_key_path.clone(),
            "provided_client_id",
        );
        let identity = build_identity(&args, environment).expect("no error expected");
        assert_eq!(identity.client_id, "provided_client_id".to_string());
        assert_eq!(identity.private_key_path, auth_private_key_path);
    }

    #[test]
    fn test_build_identity_with_token() {
        let tempdir = TempDir::new().unwrap();
        let auth_private_key_path = tempdir.path().join("private-key");

        let server = MockServer::start();

        let args = testing_args(
            "parent-client-id",
            "",
            "TOKEN",
            auth_private_key_path.clone(),
            "",
        );

        // Expect a request to create the identity
        let identity_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/identity")
                .header_includes(AUTHORIZATION.as_str(), "Bearer TOKEN");
            then.status(200)
                .body(identity_body("created_client_id", &args.organization_id));
        });

        let environment = NewRelicEnvironment::Custom {
            token_renewal_endpoint: "https://should-not-call.this"
                .parse()
                .expect("url should be valid"),
            system_identity_creation_uri: format!("{}/identity", server.base_url())
                .parse()
                .expect("url should be valid"),
        };

        let identity = build_identity(&args, environment).expect("no error expected");

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

        let args = testing_args(
            "parent-client-id",
            "client-secret-value",
            "",
            auth_private_key_path.clone(),
            "",
        );

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
                .body(identity_body("created_client_id", &args.organization_id));
        });

        let environment = NewRelicEnvironment::Custom {
            token_renewal_endpoint: format!("{}/token", server.base_url())
                .parse()
                .expect("url should be valid"),
            system_identity_creation_uri: format!("{}/identity", server.base_url())
                .parse()
                .expect("url should be valid"),
        };

        let identity = build_identity(&args, environment).expect("no error expected");

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

    fn testing_args(
        auth_parent_client_id: &str,
        auth_parent_client_secret: &str,
        auth_parent_token: &str,
        auth_private_key_path: PathBuf,
        auth_client_id: &str,
    ) -> Args {
        Args {
            output_path: Default::default(),
            fleet_enabled: true,
            region: Region::US,
            fleet_id: "some-id".to_string(),
            organization_id: "some-org".to_string(),
            agent_set: AgentSet::None,
            auth_parent_client_id: auth_parent_client_id.to_string(),
            auth_parent_client_secret: auth_parent_client_secret.to_string(),
            auth_parent_token: auth_parent_token.to_string(),
            auth_private_key_path,
            auth_client_id: auth_client_id.to_string(),
            proxy_config: None,
        }
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
