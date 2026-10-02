//! Value provider that reads secrets from Azure Key Vault via Managed Identity or Service Principal.

use crate::http::client::{HttpBuildError, HttpClient, HttpResponseError};
use crate::http::config::{HttpConfig, ProxyConfig};
use crate::utils::sensitive_string::SensitiveString;
use crate::value_provider::ValueProvider;
use duration_str::deserialize_duration;
use http::Request;
use serde::Deserialize;
use std::time::Duration;
use thiserror::Error;
use url::{ParseError, Url};
use wrapper_with_default::WrapperWithDefault;

const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

/// Azure IMDS endpoint for obtaining a Managed Identity token.
const DEFAULT_IMDS_TOKEN_URL: &str = "http://169.254.169.254/metadata/identity/oauth2/token?api-version=2018-02-01&resource=https%3A%2F%2Fvault.azure.net%2F";

/// Microsoft Entra ID (AAD) token endpoint base URL.
const DEFAULT_LOGIN_URL_BASE: &str = "https://login.microsoftonline.com";

/// OAuth2 scope for the Azure Key Vault API.
const KV_OAUTH2_SCOPE: &str = "https://vault.azure.net/.default";

/// Azure Key Vault REST API version used for secret reads.
const KV_API_VERSION: &str = "7.4";

/// Errors that can occur when interacting with Azure Key Vault.
#[derive(Debug, Error)]
pub enum AzureKeyVaultError {
    /// The HTTP client could not be built.
    #[error("could not build the http client: {0}")]
    HttpClient(#[from] HttpBuildError),

    /// The vault URL could not be parsed.
    #[error("could not parse vault url: {0}")]
    UrlParseError(#[from] ParseError),

    /// The token request (IMDS or service principal) failed with a network or HTTP error.
    #[error("token request failed: {0}")]
    TokenRequest(String),

    /// The token endpoint responded successfully but contained no access token.
    #[error("token response did not contain an access token")]
    TokenMissing,

    /// The Key Vault secret request failed with a non-404 error.
    #[error("secret request failed: {0}")]
    SecretRequest(String),

    /// The secret was not found in Key Vault (404 or null value).
    #[error("secret not found")]
    NotFound,

    /// A response body could not be deserialized.
    #[error("could not deserialize response: {0}")]
    DeserializeError(String),
}

/// Client timeout with a sensible default.
#[derive(Debug, Deserialize, Clone, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_CLIENT_TIMEOUT)]
pub struct ClientTimeout(#[serde(deserialize_with = "deserialize_duration")] Duration);

/// Authentication method for the Azure Key Vault provider.
#[derive(Debug, Default, Deserialize, PartialEq, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AzureAuth {
    /// Managed Identity via the Azure IMDS endpoint. Works only from Azure-hosted environments
    /// (VMs, AKS pods with a Managed Identity assigned, App Service, etc.).
    #[default]
    ManagedIdentity,

    /// Service Principal (client credentials flow). Works from any environment.
    ServicePrincipal {
        /// Azure tenant (directory) ID.
        tenant_id: String,
        /// Service principal application (client) ID.
        client_id: String,
        /// Service principal client secret.
        client_secret: SensitiveString,
    },
}

/// Configuration for the Azure Key Vault provider.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct AzureKeyVaultConfig {
    /// URL of the Azure Key Vault instance (e.g. `https://myvault.vault.azure.net/`).
    pub vault_url: Url,

    /// Authentication method. Defaults to `managed_identity`.
    #[serde(default)]
    pub auth: AzureAuth,

    /// Timeout applied to both connect and read phases of every HTTP request.
    #[serde(default)]
    pub(crate) client_timeout: ClientTimeout,

    /// Injected at runtime by the caller; not read from config.
    #[serde(skip)]
    pub proxy_config: ProxyConfig,
}

/// Runtime representation of the configured auth method — holds ready-to-use strings.
enum AzureAuthRuntime {
    ManagedIdentity {
        /// Full IMDS token URL including query parameters.
        imds_url: String,
    },
    ServicePrincipal {
        /// Full AAD token endpoint URL (`{login_base}/{tenant_id}/oauth2/v2.0/token`).
        token_url: String,
        client_id: String,
        client_secret: String,
    },
}

/// Shared token response shape for both IMDS and AAD OAuth2 endpoints.
#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
}

/// Azure Key Vault secret response shape (only the field we need).
#[derive(Deserialize)]
struct KeyVaultSecretResponse {
    value: Option<String>,
}

/// Azure Key Vault provider. Authenticates via Managed Identity (IMDS) or Service Principal.
pub struct AzureKeyVault {
    client: HttpClient,
    vault_url: Url,
    auth: AzureAuthRuntime,
}

impl AzureKeyVault {
    /// Builds an `AzureKeyVault` from the given configuration.
    pub fn try_build(config: AzureKeyVaultConfig) -> Result<Self, AzureKeyVaultError> {
        let http_config = HttpConfig::new(
            config.client_timeout.clone().into(),
            config.client_timeout.into(),
            config.proxy_config,
        );

        let mut vault_url = config.vault_url;
        let path = vault_url.path();
        if !path.ends_with('/') {
            vault_url.set_path(&format!("{path}/"));
        }
        let auth = match config.auth.clone() {
            AzureAuth::ManagedIdentity => AzureAuthRuntime::ManagedIdentity {
                imds_url: DEFAULT_IMDS_TOKEN_URL.to_string(),
            },
            AzureAuth::ServicePrincipal {
                tenant_id,
                client_id,
                client_secret,
            } => AzureAuthRuntime::ServicePrincipal {
                token_url: format!("{DEFAULT_LOGIN_URL_BASE}/{tenant_id}/oauth2/v2.0/token"),
                client_id,
                client_secret: client_secret.expose_secret().to_string(),
            },
        };
        Ok(Self {
            client: HttpClient::new(http_config).map_err(AzureKeyVaultError::HttpClient)?,
            vault_url,
            auth,
        })
    }

    fn get_token(&self) -> Result<String, AzureKeyVaultError> {
        match &self.auth {
            AzureAuthRuntime::ManagedIdentity { imds_url } => self.get_imds_token(imds_url),
            AzureAuthRuntime::ServicePrincipal {
                token_url,
                client_id,
                client_secret,
            } => self.get_sp_token(token_url, client_id, client_secret),
        }
    }

    fn get_imds_token(&self, imds_url: &str) -> Result<String, AzureKeyVaultError> {
        let request = Request::builder()
            .method("GET")
            .uri(imds_url)
            .header("Metadata", "true")
            .body(Vec::new())
            .map_err(|e| AzureKeyVaultError::TokenRequest(e.to_string()))?;

        let response = self.client.send(request).map_err(|e| match e {
            HttpResponseError::UnsuccessfulResponse { status_code, body } => {
                AzureKeyVaultError::TokenRequest(format!(
                    "IMDS responded with status {status_code}: {}",
                    String::from_utf8_lossy(&body)
                ))
            }
            _ => AzureKeyVaultError::TokenRequest(e.to_string()),
        })?;

        let body = String::from_utf8(response.into_body())
            .map_err(|e| AzureKeyVaultError::DeserializeError(format!("invalid utf8: {e}")))?;

        let token_response: TokenResponse = serde_json::from_str(&body)
            .map_err(|e| AzureKeyVaultError::DeserializeError(e.to_string()))?;

        token_response
            .access_token
            .filter(|t| !t.is_empty())
            .ok_or(AzureKeyVaultError::TokenMissing)
    }

    fn get_sp_token(
        &self,
        token_url: &str,
        client_id: &str,
        client_secret: &str,
    ) -> Result<String, AzureKeyVaultError> {
        let body: String = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", "client_credentials")
            .append_pair("client_id", client_id)
            .append_pair("client_secret", client_secret)
            .append_pair("scope", KV_OAUTH2_SCOPE)
            .finish();

        let request = Request::builder()
            .method("POST")
            .uri(token_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body.into_bytes())
            .map_err(|e| AzureKeyVaultError::TokenRequest(e.to_string()))?;

        let response = self.client.send(request).map_err(|e| match e {
            HttpResponseError::UnsuccessfulResponse { status_code, body } => {
                AzureKeyVaultError::TokenRequest(format!(
                    "token endpoint responded with status {status_code}: {}",
                    String::from_utf8_lossy(&body)
                ))
            }
            _ => AzureKeyVaultError::TokenRequest(e.to_string()),
        })?;

        let body = String::from_utf8(response.into_body())
            .map_err(|e| AzureKeyVaultError::DeserializeError(format!("invalid utf8: {e}")))?;

        let token_response: TokenResponse = serde_json::from_str(&body)
            .map_err(|e| AzureKeyVaultError::DeserializeError(e.to_string()))?;

        token_response
            .access_token
            .filter(|t| !t.is_empty())
            .ok_or(AzureKeyVaultError::TokenMissing)
    }
}

impl ValueProvider for AzureKeyVault {
    type Error = AzureKeyVaultError;

    fn get_value(&self, secret_name: &str) -> Result<String, Self::Error> {
        let token = self.get_token()?;

        let secret_url = self
            .vault_url
            .join(&format!(
                "secrets/{secret_name}?api-version={KV_API_VERSION}"
            ))
            .map_err(AzureKeyVaultError::UrlParseError)?;

        let request = Request::builder()
            .method("GET")
            .uri(secret_url.as_str())
            .header("Authorization", format!("Bearer {token}"))
            .body(Vec::new())
            .map_err(|e| AzureKeyVaultError::SecretRequest(e.to_string()))?;

        let response = self.client.send(request).map_err(|e| match e {
            HttpResponseError::UnsuccessfulResponse { status_code, .. }
                if status_code.as_u16() == 404 =>
            {
                AzureKeyVaultError::NotFound
            }
            HttpResponseError::UnsuccessfulResponse { status_code, body } => {
                AzureKeyVaultError::SecretRequest(format!(
                    "key vault responded with status {status_code}: {}",
                    String::from_utf8_lossy(&body)
                ))
            }
            _ => AzureKeyVaultError::SecretRequest(e.to_string()),
        })?;

        let body = String::from_utf8(response.into_body())
            .map_err(|e| AzureKeyVaultError::DeserializeError(format!("invalid utf8: {e}")))?;

        let secret_response: KeyVaultSecretResponse = serde_json::from_str(&body)
            .map_err(|e| AzureKeyVaultError::DeserializeError(e.to_string()))?;

        secret_response
            .value
            .filter(|v| !v.is_empty())
            .ok_or(AzureKeyVaultError::NotFound)
    }
}

#[cfg(test)]
#[allow(missing_docs)]
pub mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use httpmock::Method::{GET, POST};
    use httpmock::MockServer;

    impl AzureKeyVault {
        fn with_imds_url(mut self, url: String) -> Self {
            if let AzureAuthRuntime::ManagedIdentity { ref mut imds_url } = self.auth {
                *imds_url = url;
            }
            self
        }

        fn with_sp_token_url(mut self, url: String) -> Self {
            if let AzureAuthRuntime::ServicePrincipal {
                ref mut token_url, ..
            } = self.auth
            {
                *token_url = url;
            }
            self
        }
    }

    impl Default for AzureKeyVaultConfig {
        fn default() -> Self {
            Self {
                vault_url: Url::parse("https://placeholder.vault.azure.net/").unwrap(),
                auth: AzureAuth::default(),
                client_timeout: ClientTimeout::default(),
                proxy_config: ProxyConfig::default(),
            }
        }
    }

    // ── Managed Identity helpers ─────────────────────────────────────────────

    const IMDS_PATH: &str = "/metadata/identity/oauth2/token";
    const IMDS_RESPONSE: &str =
        r#"{"access_token":"test-bearer-token","expires_in":"3599","token_type":"Bearer"}"#;

    fn build_mi_provider(vault_server: &MockServer, imds_server: &MockServer) -> AzureKeyVault {
        let config = AzureKeyVaultConfig {
            vault_url: Url::parse(&vault_server.base_url()).unwrap(),
            ..Default::default()
        };
        AzureKeyVault::try_build(config)
            .unwrap()
            .with_imds_url(imds_server.url(IMDS_PATH))
    }

    // ── Service Principal helpers ────────────────────────────────────────────

    const SP_TOKEN_PATH: &str = "/test-tenant/oauth2/v2.0/token";
    const SP_TOKEN_RESPONSE: &str =
        r#"{"access_token":"sp-bearer-token","token_type":"Bearer","expires_in":3599}"#;

    fn build_sp_provider(vault_server: &MockServer, token_server: &MockServer) -> AzureKeyVault {
        let config = AzureKeyVaultConfig {
            vault_url: Url::parse(&vault_server.base_url()).unwrap(),
            auth: AzureAuth::ServicePrincipal {
                tenant_id: "test-tenant".to_string(),
                client_id: "test-client-id".to_string(),
                client_secret: "test-client-secret".into(),
            },
            ..Default::default()
        };
        AzureKeyVault::try_build(config)
            .unwrap()
            .with_sp_token_url(token_server.url(SP_TOKEN_PATH))
    }

    // ── Managed Identity tests ───────────────────────────────────────────────

    #[test]
    fn test_mi_get_secret_success() {
        let vault_server = MockServer::start();
        let imds_server = MockServer::start();

        imds_server.mock(|when, then| {
            when.method(GET).path(IMDS_PATH);
            then.status(200).body(IMDS_RESPONSE);
        });
        vault_server.mock(|when, then| {
            when.method(GET).path("/secrets/my-secret");
            then.status(200).body(r#"{"value":"super-secret-value"}"#);
        });

        let provider = build_mi_provider(&vault_server, &imds_server);
        assert_eq!(
            provider.get_value("my-secret").unwrap(),
            "super-secret-value"
        );
    }

    #[test]
    fn test_mi_secret_not_found() {
        let vault_server = MockServer::start();
        let imds_server = MockServer::start();

        imds_server.mock(|when, then| {
            when.method(GET).path(IMDS_PATH);
            then.status(200).body(IMDS_RESPONSE);
        });
        vault_server.mock(|when, then| {
            when.method(GET).path("/secrets/missing");
            then.status(404);
        });

        let provider = build_mi_provider(&vault_server, &imds_server);
        assert_matches!(
            provider.get_value("missing"),
            Err(AzureKeyVaultError::NotFound)
        );
    }

    #[test]
    fn test_mi_imds_non_success_status() {
        let vault_server = MockServer::start();
        let imds_server = MockServer::start();

        imds_server.mock(|when, then| {
            when.method(GET).path(IMDS_PATH);
            then.status(500).body("internal error");
        });

        let provider = build_mi_provider(&vault_server, &imds_server);
        assert_matches!(
            provider.get_value("any-secret"),
            Err(AzureKeyVaultError::TokenRequest(_))
        );
    }

    #[test]
    fn test_mi_imds_token_missing_in_response() {
        let vault_server = MockServer::start();
        let imds_server = MockServer::start();

        imds_server.mock(|when, then| {
            when.method(GET).path(IMDS_PATH);
            then.status(200).body(r#"{"access_token":null}"#);
        });

        let provider = build_mi_provider(&vault_server, &imds_server);
        assert_matches!(
            provider.get_value("any-secret"),
            Err(AzureKeyVaultError::TokenMissing)
        );
    }

    #[test]
    fn test_mi_imds_invalid_json() {
        let vault_server = MockServer::start();
        let imds_server = MockServer::start();

        imds_server.mock(|when, then| {
            when.method(GET).path(IMDS_PATH);
            then.status(200).body("not json at all");
        });

        let provider = build_mi_provider(&vault_server, &imds_server);
        assert_matches!(
            provider.get_value("any-secret"),
            Err(AzureKeyVaultError::DeserializeError(_))
        );
    }

    #[test]
    fn test_mi_connection_failed() {
        let config = AzureKeyVaultConfig {
            vault_url: Url::parse("http://127.0.0.1:1").unwrap(),
            ..Default::default()
        };
        let provider = AzureKeyVault::try_build(config)
            .unwrap()
            .with_imds_url("http://127.0.0.1:1/metadata/identity/oauth2/token".to_string());

        assert_matches!(
            provider.get_value("any-secret"),
            Err(AzureKeyVaultError::TokenRequest(_))
        );
    }

    #[test]
    fn test_mi_secret_value_null() {
        let vault_server = MockServer::start();
        let imds_server = MockServer::start();

        imds_server.mock(|when, then| {
            when.method(GET).path(IMDS_PATH);
            then.status(200).body(IMDS_RESPONSE);
        });
        vault_server.mock(|when, then| {
            when.method(GET).path("/secrets/my-secret");
            then.status(200).body(r#"{"value":null}"#);
        });

        let provider = build_mi_provider(&vault_server, &imds_server);
        assert_matches!(
            provider.get_value("my-secret"),
            Err(AzureKeyVaultError::NotFound)
        );
    }

    #[test]
    fn test_mi_kv_non_404_error() {
        let vault_server = MockServer::start();
        let imds_server = MockServer::start();

        imds_server.mock(|when, then| {
            when.method(GET).path(IMDS_PATH);
            then.status(200).body(IMDS_RESPONSE);
        });
        vault_server.mock(|when, then| {
            when.method(GET).path("/secrets/my-secret");
            then.status(403).body("Forbidden");
        });

        let provider = build_mi_provider(&vault_server, &imds_server);
        assert_matches!(
            provider.get_value("my-secret"),
            Err(AzureKeyVaultError::SecretRequest(_))
        );
    }

    #[test]
    fn test_vault_url_trailing_slash_normalised() {
        let config = AzureKeyVaultConfig {
            vault_url: Url::parse("https://myvault.vault.azure.net").unwrap(),
            ..Default::default()
        };
        let provider = AzureKeyVault::try_build(config).unwrap();
        assert!(provider.vault_url.as_str().ends_with('/'));
    }

    // ── Service Principal tests ──────────────────────────────────────────────

    #[test]
    fn test_sp_get_secret_success() {
        let vault_server = MockServer::start();
        let token_server = MockServer::start();

        token_server.mock(|when, then| {
            when.method(POST).path(SP_TOKEN_PATH);
            then.status(200).body(SP_TOKEN_RESPONSE);
        });
        vault_server.mock(|when, then| {
            when.method(GET).path("/secrets/db-password");
            then.status(200).body(r#"{"value":"hunter2"}"#);
        });

        let provider = build_sp_provider(&vault_server, &token_server);
        assert_eq!(provider.get_value("db-password").unwrap(), "hunter2");
    }

    #[test]
    fn test_sp_token_request_fails() {
        let vault_server = MockServer::start();
        let token_server = MockServer::start();

        token_server.mock(|when, then| {
            when.method(POST).path(SP_TOKEN_PATH);
            then.status(401).body(r#"{"error":"invalid_client"}"#);
        });

        let provider = build_sp_provider(&vault_server, &token_server);
        assert_matches!(
            provider.get_value("any-secret"),
            Err(AzureKeyVaultError::TokenRequest(_))
        );
    }

    #[test]
    fn test_sp_token_missing_in_response() {
        let vault_server = MockServer::start();
        let token_server = MockServer::start();

        token_server.mock(|when, then| {
            when.method(POST).path(SP_TOKEN_PATH);
            then.status(200).body(r#"{"access_token":null}"#);
        });

        let provider = build_sp_provider(&vault_server, &token_server);
        assert_matches!(
            provider.get_value("any-secret"),
            Err(AzureKeyVaultError::TokenMissing)
        );
    }

    #[test]
    fn test_sp_secret_not_found() {
        let vault_server = MockServer::start();
        let token_server = MockServer::start();

        token_server.mock(|when, then| {
            when.method(POST).path(SP_TOKEN_PATH);
            then.status(200).body(SP_TOKEN_RESPONSE);
        });
        vault_server.mock(|when, then| {
            when.method(GET).path("/secrets/missing");
            then.status(404);
        });

        let provider = build_sp_provider(&vault_server, &token_server);
        assert_matches!(
            provider.get_value("missing"),
            Err(AzureKeyVaultError::NotFound)
        );
    }

    #[test]
    fn test_sp_token_request_sends_correct_form_body() {
        let vault_server = MockServer::start();
        let token_server = MockServer::start();

        token_server.mock(|when, then| {
            when.method(POST)
                .path(SP_TOKEN_PATH)
                .body_includes("grant_type=client_credentials")
                .body_includes("client_id=test-client-id")
                .body_includes("client_secret=test-client-secret")
                .body_includes("scope=https%3A%2F%2Fvault.azure.net%2F.default");
            then.status(200).body(SP_TOKEN_RESPONSE);
        });
        vault_server.mock(|when, then| {
            when.method(GET).path("/secrets/any");
            then.status(200).body(r#"{"value":"val"}"#);
        });

        let provider = build_sp_provider(&vault_server, &token_server);
        assert!(provider.get_value("any").is_ok());
    }
}
