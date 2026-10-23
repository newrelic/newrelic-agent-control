//! Value provider that reads secrets from Google Cloud Secret Manager using the metadata server of a
//! Google Cloud workload or service account key credentials.

use crate::http::client::{HttpBuildError, HttpClient, HttpResponseError};
use crate::http::config::{HttpConfig, ProxyConfig};
use crate::utils::sensitive_string::SensitiveString;
use crate::value_provider::ValueProvider;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use duration_str::deserialize_duration;
use http::Request;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use url::Url;
use wrapper_with_default::WrapperWithDefault;

const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

/// Default Secret Manager API endpoint.
const DEFAULT_ENDPOINT: &str = "https://secretmanager.googleapis.com/";

/// Metadata server endpoint for obtaining an access token of the default service account.
const DEFAULT_METADATA_TOKEN_URL: &str =
    "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token";

/// Google OAuth2 token endpoint, used for the service account JWT bearer flow.
const DEFAULT_OAUTH2_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// OAuth2 scope required to access secrets.
const OAUTH2_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

const JWT_BEARER_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

/// Lifetime of the signed JWT assertion. It is exchanged right away, so it is the maximum Google allows.
const ASSERTION_LIFETIME: Duration = Duration::from_secs(3600);

/// Version read when the secret path does not specify one.
const LATEST_VERSION: &str = "latest";

/// Errors that can occur when interacting with Google Cloud Secret Manager.
#[derive(Debug, Error)]
pub enum GcpSecretManagerError {
    /// The HTTP client could not be built.
    #[error("could not build the http client: {0}")]
    HttpClient(#[from] HttpBuildError),

    /// The service account key file could not be read, parsed or does not hold a valid RSA key.
    #[error("invalid service account key file: {0}")]
    KeyFile(String),

    /// The token could not be obtained: the request or the assertion signing failed, or the
    /// response held no access token.
    #[error("token request failed: {0}")]
    TokenRequest(String),

    /// The Secret Manager request failed with an error other than "not found".
    #[error("secret request failed: {0}")]
    SecretRequest(String),

    /// The secret or its version does not exist.
    #[error("secret not found")]
    NotFound,

    /// The secret has no value or it is empty.
    #[error("secret has an empty value")]
    EmptySecret,

    /// A response body could not be deserialized, or the secret is not valid UTF-8 text (binary
    /// secrets are not supported).
    #[error("could not deserialize response: {0}")]
    DeserializeError(String),

    /// The secret path did not match the expected `source:secret-name[:version]` format.
    #[error("secret path '{0}' does not have the expected format 'source:secret-name[:version]'")]
    IncorrectSecretPath(String),

    /// The requested source was not configured.
    #[error("secret source '{0}' not found")]
    SourceNotFound(String),
}

/// A parsed reference of the form `<source>:<secret-name>[:<version>]`.
struct GcpSecretPath {
    source: String,
    secret_name: String,
    version: String,
}

impl FromStr for GcpSecretPath {
    type Err = GcpSecretManagerError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let incorrect = || GcpSecretManagerError::IncorrectSecretPath(s.to_string());

        let mut parts = s.splitn(3, ':');
        let source = parts
            .next()
            .filter(|p| !p.is_empty())
            .ok_or_else(incorrect)?;
        let secret_name = parts
            .next()
            .filter(|p| is_valid_identifier(p))
            .ok_or_else(incorrect)?;
        let version = match parts.next() {
            Some(version) if is_valid_identifier(version) => version,
            Some(_) => return Err(incorrect()),
            None => LATEST_VERSION,
        };

        Ok(Self {
            source: source.to_string(),
            secret_name: secret_name.to_string(),
            version: version.to_string(),
        })
    }
}

/// Whether the value only has characters allowed in secret names and versions. It keeps the
/// value from altering the request path.
fn is_valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Client timeout with a sensible default.
#[derive(Debug, Deserialize, Clone, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_CLIENT_TIMEOUT)]
pub struct ClientTimeout(#[serde(deserialize_with = "deserialize_duration")] Duration);

/// Authentication method for a Google Cloud Secret Manager source.
#[derive(Debug, Default, Deserialize, PartialEq, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GcpAuth {
    /// Access token of the service account attached to the workload, retrieved from the metadata
    /// server. Works only from Google Cloud environments (Compute Engine, GKE with Workload Identity, etc.).
    #[default]
    MetadataServer,

    /// Service account key credentials (JWT bearer flow). Works from any environment.
    ServiceAccount {
        /// Path to the service account JSON key file, as downloaded from Google Cloud. Only its
        /// `client_email` and `private_key` fields are used. It is read once at startup.
        key_file: PathBuf,
    },
}

/// Fields used from the service account JSON key file.
#[derive(Deserialize)]
struct ServiceAccountKey {
    client_email: String,
    private_key: SensitiveString,
}

/// Configuration for a single Google Cloud Secret Manager source.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct GcpSecretManagerSourceConfig {
    /// Id or number of the Google Cloud project holding the secrets.
    pub project: String,

    /// Overrides the default `https://secretmanager.googleapis.com/` endpoint,
    /// for example to use a regional or Private Service Connect endpoint.
    #[serde(default)]
    pub endpoint: Option<Url>,

    /// Authentication method. Defaults to `metadata_server`.
    #[serde(default)]
    pub auth: GcpAuth,
}

/// Configuration for the Google Cloud Secret Manager provider: a set of named sources and HTTP client settings.
#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct GcpSecretManagerConfig {
    /// Named Google Cloud Secret Manager sources.
    pub sources: HashMap<String, GcpSecretManagerSourceConfig>,

    /// Timeout applied to both connect and read phases of every HTTP request.
    #[serde(default)]
    pub(crate) client_timeout: ClientTimeout,

    /// Injected at runtime by the caller; not read from config.
    #[serde(skip)]
    pub proxy_config: ProxyConfig,
}

/// Ready-to-use representation of one configured auth method.
enum GcpAuthRuntime {
    MetadataServer {
        /// Full metadata server token URL.
        token_url: String,
    },
    ServiceAccount {
        /// OAuth2 token endpoint URL.
        token_url: String,
        client_email: String,
        key: EncodingKey,
    },
}

/// Runtime representation of a single named source.
struct GcpSecretManagerSource {
    project: String,
    endpoint: String,
    auth: GcpAuthRuntime,
}

/// Shared token response shape for both the metadata server and the OAuth2 endpoint.
#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
}

/// `AccessSecretVersion` response shape (only the fields we need).
#[derive(Deserialize)]
struct AccessSecretVersionResponse {
    payload: Option<SecretPayload>,
}

#[derive(Deserialize)]
struct SecretPayload {
    /// Base64 encoded secret data.
    data: Option<String>,
}

/// Claims of the service account JWT assertion.
#[derive(Serialize)]
struct AssertionClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: u64,
    exp: u64,
}

/// Google Cloud Secret Manager provider. Holds shared HTTP clients and one runtime source per configured name.
pub struct GcpSecretManager {
    client: HttpClient,
    /// Dedicated client without proxy: the metadata server is link-local and unreachable through a proxy.
    metadata_client: HttpClient,
    sources: HashMap<String, GcpSecretManagerSource>,
}

impl GcpSecretManager {
    /// Builds a `GcpSecretManager` from the given configuration.
    pub fn try_build(config: GcpSecretManagerConfig) -> Result<Self, GcpSecretManagerError> {
        let timeout: Duration = config.client_timeout.into();
        let http_config = HttpConfig::new(timeout, timeout, config.proxy_config);
        // We use default (empty) proxy config because the metadata server is only reachable from the workload itself.
        let metadata_http_config = HttpConfig::new(timeout, timeout, ProxyConfig::default());

        let sources = config
            .sources
            .into_iter()
            .map(|(name, source_config)| {
                let source = GcpSecretManagerSource::try_new(source_config)?;
                Ok((name, source))
            })
            .collect::<Result<HashMap<_, _>, GcpSecretManagerError>>()?;

        Ok(Self {
            client: HttpClient::new(http_config)?,
            metadata_client: HttpClient::new(metadata_http_config)?,
            sources,
        })
    }

    fn get_token(&self, source: &GcpSecretManagerSource) -> Result<String, GcpSecretManagerError> {
        match &source.auth {
            GcpAuthRuntime::MetadataServer { token_url } => self.get_metadata_token(token_url),
            GcpAuthRuntime::ServiceAccount {
                token_url,
                client_email,
                key,
            } => self.get_service_account_token(token_url, client_email, key),
        }
    }

    fn get_metadata_token(&self, token_url: &str) -> Result<String, GcpSecretManagerError> {
        let request = Request::builder()
            .method("GET")
            .uri(token_url)
            .header("Metadata-Flavor", "Google")
            .body(Vec::new())
            .map_err(|e| GcpSecretManagerError::TokenRequest(e.to_string()))?;

        let response = self
            .metadata_client
            .send(request)
            .map_err(|e| token_request_error("metadata server", e))?;

        parse_token_response(response.into_body())
    }

    fn get_service_account_token(
        &self,
        token_url: &str,
        client_email: &str,
        key: &EncodingKey,
    ) -> Result<String, GcpSecretManagerError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| GcpSecretManagerError::TokenRequest(e.to_string()))?
            .as_secs();
        let claims = AssertionClaims {
            iss: client_email,
            scope: OAUTH2_SCOPE,
            aud: token_url,
            iat: now,
            exp: now + ASSERTION_LIFETIME.as_secs(),
        };
        let assertion = jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, key)
            .map_err(|e| GcpSecretManagerError::TokenRequest(e.to_string()))?;

        let body: String = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", JWT_BEARER_GRANT_TYPE)
            .append_pair("assertion", &assertion)
            .finish();

        let request = Request::builder()
            .method("POST")
            .uri(token_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body.into_bytes())
            .map_err(|e| GcpSecretManagerError::TokenRequest(e.to_string()))?;

        let response = self
            .client
            .send(request)
            .map_err(|e| token_request_error("token endpoint", e))?;

        parse_token_response(response.into_body())
    }
}

fn token_request_error(origin: &str, error: HttpResponseError) -> GcpSecretManagerError {
    match error {
        HttpResponseError::UnsuccessfulResponse { status_code, body } => {
            GcpSecretManagerError::TokenRequest(format!(
                "{origin} responded with status {status_code}: {}",
                String::from_utf8_lossy(&body)
            ))
        }
        _ => GcpSecretManagerError::TokenRequest(error.to_string()),
    }
}

fn parse_token_response(body: Vec<u8>) -> Result<String, GcpSecretManagerError> {
    let body = String::from_utf8(body)
        .map_err(|e| GcpSecretManagerError::DeserializeError(format!("invalid utf8: {e}")))?;

    let token_response: TokenResponse = serde_json::from_str(&body)
        .map_err(|e| GcpSecretManagerError::DeserializeError(e.to_string()))?;

    token_response
        .access_token
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            GcpSecretManagerError::TokenRequest(
                "response did not contain an access token".to_string(),
            )
        })
}

fn read_key_file(path: &Path) -> Result<ServiceAccountKey, GcpSecretManagerError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        GcpSecretManagerError::KeyFile(format!("could not read '{}': {e}", path.display()))
    })?;
    serde_json::from_str(&content).map_err(|e| {
        GcpSecretManagerError::KeyFile(format!("could not parse '{}': {e}", path.display()))
    })
}

impl GcpSecretManagerSource {
    fn try_new(config: GcpSecretManagerSourceConfig) -> Result<Self, GcpSecretManagerError> {
        let mut endpoint = config
            .endpoint
            .map_or_else(|| DEFAULT_ENDPOINT.to_string(), String::from);
        if !endpoint.ends_with('/') {
            endpoint.push('/');
        }

        let auth = match config.auth {
            GcpAuth::MetadataServer => GcpAuthRuntime::MetadataServer {
                token_url: DEFAULT_METADATA_TOKEN_URL.to_string(),
            },
            GcpAuth::ServiceAccount { key_file } => {
                let ServiceAccountKey {
                    client_email,
                    private_key,
                } = read_key_file(&key_file)?;
                GcpAuthRuntime::ServiceAccount {
                    token_url: DEFAULT_OAUTH2_TOKEN_URL.to_string(),
                    client_email,
                    key: EncodingKey::from_rsa_pem(private_key.expose_secret().as_bytes())
                        .map_err(|e| {
                            GcpSecretManagerError::KeyFile(format!("invalid private key: {e}"))
                        })?,
                }
            }
        };

        Ok(Self {
            project: config.project,
            endpoint,
            auth,
        })
    }

    /// URL of the `AccessSecretVersion` method for the given secret and version.
    fn access_url(&self, secret_name: &str, version: &str) -> String {
        format!(
            "{}v1/projects/{}/secrets/{secret_name}/versions/{version}:access",
            self.endpoint, self.project
        )
    }
}

impl ValueProvider for GcpSecretManager {
    type Error = GcpSecretManagerError;

    fn get_value(&self, secret_path: &str) -> Result<String, Self::Error> {
        let GcpSecretPath {
            source: source_name,
            secret_name,
            version,
        } = GcpSecretPath::from_str(secret_path)?;

        let source = self
            .sources
            .get(&source_name)
            .ok_or_else(|| GcpSecretManagerError::SourceNotFound(source_name.clone()))?;

        let token = self.get_token(source)?;

        let request = Request::builder()
            .method("GET")
            .uri(source.access_url(&secret_name, &version))
            .header("Authorization", format!("Bearer {token}"))
            .body(Vec::new())
            .map_err(|e| GcpSecretManagerError::SecretRequest(e.to_string()))?;

        let response = self.client.send(request).map_err(|e| match e {
            HttpResponseError::UnsuccessfulResponse { status_code, .. }
                if status_code.as_u16() == 404 =>
            {
                GcpSecretManagerError::NotFound
            }
            HttpResponseError::UnsuccessfulResponse { status_code, body } => {
                GcpSecretManagerError::SecretRequest(format!(
                    "secret manager responded with status {status_code}: {}",
                    String::from_utf8_lossy(&body)
                ))
            }
            _ => GcpSecretManagerError::SecretRequest(e.to_string()),
        })?;

        parse_secret_response(response.into_body())
    }
}

/// Extracts the text secret from an `AccessSecretVersion` response body.
fn parse_secret_response(body: Vec<u8>) -> Result<String, GcpSecretManagerError> {
    let body = String::from_utf8(body)
        .map_err(|e| GcpSecretManagerError::DeserializeError(format!("invalid utf8: {e}")))?;

    let secret: AccessSecretVersionResponse = serde_json::from_str(&body)
        .map_err(|e| GcpSecretManagerError::DeserializeError(e.to_string()))?;

    let data = secret
        .payload
        .and_then(|p| p.data)
        .filter(|d| !d.is_empty())
        .ok_or(GcpSecretManagerError::EmptySecret)?;
    let decoded = STANDARD
        .decode(data)
        .map_err(|e| GcpSecretManagerError::DeserializeError(format!("invalid base64: {e}")))?;
    let value = String::from_utf8(decoded).map_err(|_| {
        GcpSecretManagerError::DeserializeError("secret is not valid utf8 text".to_string())
    })?;

    if value.is_empty() {
        return Err(GcpSecretManagerError::EmptySecret);
    }
    Ok(value)
}

#[cfg(test)]
#[allow(missing_docs)]
pub mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use httpmock::Method::{GET, POST};
    use httpmock::MockServer;
    use mockall::mock;
    use rstest::rstest;
    use tempfile::TempDir;

    mock! {
        pub GcpSecretManager {}

        impl ValueProvider for GcpSecretManager {
            type Error = GcpSecretManagerError;

            fn get_value(&self, path: &str) -> Result<String, GcpSecretManagerError>;
        }
    }

    const SOURCE_NAME: &str = "test-source";
    const PROJECT: &str = "my-project";
    const METADATA_TOKEN_PATH: &str = "/token";
    const OAUTH2_TOKEN_PATH: &str = "/oauth2/token";
    const ACCESS_PATH: &str = "/v1/projects/my-project/secrets/my-secret/versions/latest:access";
    const CLIENT_EMAIL: &str = "ac@my-project.iam.gserviceaccount.com";
    const TOKEN_RESPONSE: &str = r#"{"access_token":"test-bearer-token","expires_in":3599}"#;
    /// Throwaway RSA key used only by these tests.
    const TEST_PRIVATE_KEY: &str = "\
-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDjdPl5vlDHfrBi
Cpb8evDaPnMf0CyvoJV8UWo5QXrr6Tsh83hXJdL/ATw5KcyyPu6SzKgBmZtIbeci
Sh6//cRksRgR/xn4OZBSJ+NhbDwWDITqd9XdQDrRdIxFayHAzmlrWLuDtjiFMiRY
99BuYa6vutSLNkTV9mA6u+o85YpliBBPufCFBJrfDnaAudrPb/3aVlcnyVPLrWj4
7DGTNi4SVkr2Bnn7qieMM4XxuJSSrTGai7+Wi7VEOh3Nyk7vFcdGXkDJm7AL5DmX
3N03QKRIUvTYBg2ntydrBvoJXGO6/GbZjkTNOH8rd16qwS/Rb+XDxiVC+fbgyyeQ
tokP6ZBDAgMBAAECggEAELb33kpuBTr6GzpL/iHKsT8FlsDlh8J1Ob2B4NM/BTcO
cWBC56SP7M1ab5jyweZ+AauobPWsw5XP9mUb/V/Yns2kxy3H6sXQJTlpLC+X+sbQ
vaowvSGLMPkvlapZwKFqfcj3fqZVYPReV2telb336FICbHiAdni0eZx1iiQUF4HU
r1dGcaFplEL0QL/BwR08ULrJUropReu2fn8MIfJgrHLkTPGquj+i+JSI67xexOev
WBv/plap+vuxTb+EGhUBsQoAA3yxS5akf5p1o1bsTB9gSoKKgQasQoyJ2bsXsssa
AmIoxHKPbDuVz7Ic4UzXcXaSjGko6ji5W6yCo4/5gQKBgQD9NItDGS1L8Il3Ickq
7DNFEzlZrpxzRgE4+2ojM2XdCHNiKDCBKPEsqOufbiMblOg+wKOa/qyEp+z8LjeZ
8NzT9bRtKjETK9qfNnKddAW+MV9AZhSqBEU4xbNpPkbPO33Knt751yxUu56puED/
Ckw7LvnhPlttqyReVF0qSSeqdQKBgQDl960YHFO8WCpkYiEISw07Ax8B26XcQ+tQ
nfUor9b7M18K1SxQpD6n8+RP9h6x6CnunBFnqIvS99qC7cZ9M1HW7WJULnmq5Lfz
WdDheWtlgKTMlYNojUUABB1TOWLmQhwfVvRw4eJJHnbKdegiI1OuEsbzD+dlTxRP
YXWGvvbI1wKBgQCaahPPrKf6GsvKcjk7SuaNZobgEWNpzTRrqq90niCt5jxvjPDf
o7dItA9VoBYa+PmVY2ei21McZ0hYFltkLwaM4NfShxt8hI8D73GAYkQqU/Ox008g
zA/m6nJaaVdHhBFXowF6PeXUGsNM34gLwRzyLtjGq3+YZvl76VkTs59UDQKBgGqu
6sMLqSDv+cKV72MMhjfBK+pNvnUfoRPGI4xbfq0MWOQKnsNOXq7Fqz4ZHIZGAOXr
yvs+qpcVWm5w5isA2s+NUpdg9Bp+foPLP+bJ0DPHaSxrlAm1t7PZNM1+Cs5tDuTP
DxkrATIQiuVDq4vivZg8p8TstNk/xVlKCh6Xip8DAoGAIHPJSRhDH8q03+cCvnE0
VB+MeCJnb+vFVK4XxMefmY0ZWQpEbKqoY9Ub/WuHTqE1HIvYBL0uZeQL/KtmVE7P
vVvut+SRzl9jZQjXzTuMlTVuHkR3uOb1qbQ7dkSbVr/KpTVSPFLRYadATTMj3YO7
9pGMw/rG27nMxHxzOROEfu8=
-----END PRIVATE KEY-----
";

    /// Asserts that both errors are the same variant, ignoring their payload.
    fn assert_same_variant(actual: &GcpSecretManagerError, expected: &GcpSecretManagerError) {
        assert_eq!(
            std::mem::discriminant(actual),
            std::mem::discriminant(expected),
            "unexpected error: {actual}"
        );
    }

    /// Secret response with the base64 encoding of `value`.
    fn secret_body(value: &[u8]) -> String {
        format!(r#"{{"payload":{{"data":"{}"}}}}"#, STANDARD.encode(value))
    }

    fn source_config(endpoint: Option<&str>, auth: GcpAuth) -> GcpSecretManagerSourceConfig {
        GcpSecretManagerSourceConfig {
            project: PROJECT.to_string(),
            endpoint: endpoint.map(|e| Url::parse(e).unwrap()),
            auth,
        }
    }

    /// Writes a service account key file with the given private key.
    fn write_key_file(dir: &TempDir, private_key: &str) -> PathBuf {
        let path = dir.path().join("key.json");
        let key = serde_json::json!({
            "type": "service_account",
            "client_email": CLIENT_EMAIL,
            "private_key": private_key,
        });
        std::fs::write(&path, key.to_string()).unwrap();
        path
    }

    fn service_account_auth(dir: &TempDir) -> GcpAuth {
        GcpAuth::ServiceAccount {
            key_file: write_key_file(dir, TEST_PRIVATE_KEY),
        }
    }

    fn build_provider(sources: Vec<(&str, GcpSecretManagerSourceConfig)>) -> GcpSecretManager {
        let config = GcpSecretManagerConfig {
            sources: sources
                .into_iter()
                .map(|(name, source)| (name.to_string(), source))
                .collect(),
            ..Default::default()
        };
        GcpSecretManager::try_build(config).unwrap()
    }

    /// Builds a single source provider whose token requests go to `token_server`.
    fn build_provider_with_token_server(
        secrets_server: &MockServer,
        token_server: &MockServer,
        auth: GcpAuth,
    ) -> GcpSecretManager {
        let mut provider = build_provider(vec![(
            SOURCE_NAME,
            source_config(Some(&secrets_server.base_url()), auth),
        )]);
        redirect_token_url(&mut provider, SOURCE_NAME, token_server);
        provider
    }

    fn redirect_token_url(provider: &mut GcpSecretManager, source: &str, server: &MockServer) {
        match &mut provider.sources.get_mut(source).unwrap().auth {
            GcpAuthRuntime::MetadataServer { token_url } => {
                *token_url = server.url(METADATA_TOKEN_PATH)
            }
            GcpAuthRuntime::ServiceAccount { token_url, .. } => {
                *token_url = server.url(OAUTH2_TOKEN_PATH)
            }
        }
    }

    fn mock_metadata_token(server: &MockServer) {
        server.mock(|when, then| {
            when.method(GET)
                .path(METADATA_TOKEN_PATH)
                .header("Metadata-Flavor", "Google");
            then.status(200).body(TOKEN_RESPONSE);
        });
    }

    /// Provider authenticating against a mock metadata server, which is returned to keep it alive.
    fn metadata_provider(secrets_server: &MockServer) -> (GcpSecretManager, MockServer) {
        let token_server = MockServer::start();
        mock_metadata_token(&token_server);
        let provider = build_provider_with_token_server(
            secrets_server,
            &token_server,
            GcpAuth::MetadataServer,
        );
        (provider, token_server)
    }

    fn path(secret: &str) -> String {
        format!("{SOURCE_NAME}:{secret}")
    }

    #[rstest]
    #[case::metadata_server(false)]
    #[case::service_account(true)]
    fn test_get_secret_success(#[case] service_account: bool) {
        let secrets_server = MockServer::start();
        let token_server = MockServer::start();
        let key_dir = TempDir::new().unwrap();
        let auth = if service_account {
            token_server.mock(|when, then| {
                when.method(POST)
                    .path(OAUTH2_TOKEN_PATH)
                    .form_urlencoded_tuple("grant_type", JWT_BEARER_GRANT_TYPE)
                    .form_urlencoded_tuple_exists("assertion");
                then.status(200).body(TOKEN_RESPONSE);
            });
            service_account_auth(&key_dir)
        } else {
            mock_metadata_token(&token_server);
            GcpAuth::MetadataServer
        };
        let mock = secrets_server.mock(|when, then| {
            when.method(GET)
                .path(ACCESS_PATH)
                .header("authorization", "Bearer test-bearer-token");
            then.status(200).body(secret_body(b"super-secret-value"));
        });

        let provider = build_provider_with_token_server(&secrets_server, &token_server, auth);
        assert_eq!(
            provider.get_value(&path("my-secret")).unwrap(),
            "super-secret-value"
        );
        mock.assert();
    }

    #[rstest]
    #[case::not_found(404, r#"{"error":{"status":"NOT_FOUND"}}"#.to_string(), GcpSecretManagerError::NotFound)]
    #[case::permission_denied(403, r#"{"error":{"status":"PERMISSION_DENIED"}}"#.to_string(), GcpSecretManagerError::SecretRequest(String::new()))]
    #[case::empty_value(200, secret_body(b""), GcpSecretManagerError::EmptySecret)]
    #[case::no_payload(200, "{}".to_string(), GcpSecretManagerError::EmptySecret)]
    #[case::binary_value(200, secret_body(&[0xff, 0xfe]), GcpSecretManagerError::DeserializeError(String::new()))]
    #[case::invalid_base64(200, r#"{"payload":{"data":"!!"}}"#.to_string(), GcpSecretManagerError::DeserializeError(String::new()))]
    #[case::invalid_json(200, "not json".to_string(), GcpSecretManagerError::DeserializeError(String::new()))]
    fn test_secret_response_errors(
        #[case] status: u16,
        #[case] body: String,
        #[case] expected: GcpSecretManagerError,
    ) {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path(ACCESS_PATH);
            then.status(status).body(body);
        });

        let (provider, _token_server) = metadata_provider(&server);
        let err = provider.get_value(&path("my-secret")).unwrap_err();
        assert_same_variant(&err, &expected);
    }

    #[rstest]
    #[case::request_failed(
        500,
        "internal error",
        GcpSecretManagerError::TokenRequest(String::new())
    )]
    #[case::no_access_token(
        200,
        r#"{"expires_in":3599}"#,
        GcpSecretManagerError::TokenRequest(String::new())
    )]
    fn test_token_errors(
        #[case] status: u16,
        #[case] body: &str,
        #[case] expected: GcpSecretManagerError,
    ) {
        let secrets_server = MockServer::start();
        let token_server = MockServer::start();
        token_server.mock(|when, then| {
            when.method(GET).path(METADATA_TOKEN_PATH);
            then.status(status).body(body);
        });

        let provider = build_provider_with_token_server(
            &secrets_server,
            &token_server,
            GcpAuth::MetadataServer,
        );
        let err = provider.get_value(&path("my-secret")).unwrap_err();
        assert_same_variant(&err, &expected);
    }

    #[test]
    fn test_connection_failed() {
        let provider = build_provider(vec![(
            SOURCE_NAME,
            source_config(Some("http://127.0.0.1:1"), GcpAuth::MetadataServer),
        )]);

        assert_matches!(
            provider.get_value(&path("my-secret")),
            Err(GcpSecretManagerError::TokenRequest(_))
        );
    }

    #[test]
    fn test_metadata_server_bypasses_proxy() {
        let secrets_server = MockServer::start();
        let secrets_mock = secrets_server.mock(|when, then| {
            when.method(GET).path(ACCESS_PATH);
            then.status(200).body(secret_body(b"val"));
        });

        // The proxy only forwards to the secrets server, so a metadata request sent through it would fail.
        let proxy_server = MockServer::start();
        proxy_server.proxy(|rule| {
            rule.filter(|when| {
                when.host(secrets_server.host()).port(secrets_server.port());
            });
        });

        let (mut provider, _token_server) = metadata_provider(&secrets_server);
        let timeout = Duration::from_secs(5);
        provider.client = HttpClient::new(HttpConfig::new(
            timeout,
            timeout,
            ProxyConfig::from_url(proxy_server.base_url()),
        ))
        .unwrap();

        assert_eq!(provider.get_value(&path("my-secret")).unwrap(), "val");
        secrets_mock.assert();
    }

    #[rstest]
    #[case::default_endpoint(
        None,
        "https://secretmanager.googleapis.com/v1/projects/my-project/secrets/s/versions/1:access"
    )]
    #[case::custom_endpoint_without_trailing_slash(
        Some("https://secretmanager.europe-west1.rep.googleapis.com"),
        "https://secretmanager.europe-west1.rep.googleapis.com/v1/projects/my-project/secrets/s/versions/1:access"
    )]
    fn test_access_url(#[case] endpoint: Option<&str>, #[case] expected: &str) {
        let source =
            GcpSecretManagerSource::try_new(source_config(endpoint, GcpAuth::MetadataServer))
                .unwrap();
        assert_eq!(source.access_url("s", "1"), expected);
    }

    #[rstest]
    #[case::invalid_pem(
        Some(r#"{"client_email":"ac@p.iam","private_key":"not a pem"}"#),
        GcpSecretManagerError::KeyFile(String::new())
    )]
    #[case::missing_private_key(
        Some(r#"{"client_email":"ac@p.iam"}"#),
        GcpSecretManagerError::KeyFile(String::new())
    )]
    #[case::missing_file(None, GcpSecretManagerError::KeyFile(String::new()))]
    fn test_invalid_key_file(
        #[case] content: Option<&str>,
        #[case] expected: GcpSecretManagerError,
    ) {
        let dir = TempDir::new().unwrap();
        let key_file = dir.path().join("key.json");
        if let Some(content) = content {
            std::fs::write(&key_file, content).unwrap();
        }
        let config = GcpSecretManagerConfig {
            sources: HashMap::from([(
                SOURCE_NAME.to_string(),
                source_config(None, GcpAuth::ServiceAccount { key_file }),
            )]),
            ..Default::default()
        };

        let err = GcpSecretManager::try_build(config).err().unwrap();
        assert_same_variant(&err, &expected);
    }

    #[rstest]
    #[case::latest_by_default("src:name", "src", "name", "latest")]
    #[case::explicit_version("src:name:3", "src", "name", "3")]
    #[case::name_chars("src:my_Secret-1:latest", "src", "my_Secret-1", "latest")]
    fn test_secret_path_parsing(
        #[case] input: &str,
        #[case] source: &str,
        #[case] secret_name: &str,
        #[case] version: &str,
    ) {
        let parsed = GcpSecretPath::from_str(input).unwrap();
        assert_eq!(
            (
                parsed.source.as_str(),
                parsed.secret_name.as_str(),
                parsed.version.as_str()
            ),
            (source, secret_name, version)
        );
    }

    #[rstest]
    #[case::no_colon("no-colon-here")]
    #[case::empty_source(":secret")]
    #[case::empty_secret("source:")]
    #[case::empty_version("source:secret:")]
    #[case::path_traversal("source:../other")]
    #[case::query_injection("source:secret?x=1")]
    #[case::version_traversal("source:secret:1/../2")]
    fn test_incorrect_secret_path(#[case] input: &str) {
        assert_matches!(
            GcpSecretPath::from_str(input).err(),
            Some(GcpSecretManagerError::IncorrectSecretPath(_))
        );
    }

    #[test]
    fn test_source_not_found() {
        let provider = build_provider(vec![(
            SOURCE_NAME,
            source_config(None, GcpAuth::MetadataServer),
        )]);

        assert_matches!(
            provider.get_value("nonexistent-source:my-secret"),
            Err(GcpSecretManagerError::SourceNotFound(name)) if name == "nonexistent-source"
        );
    }

    #[test]
    fn test_multiple_sources() {
        let token_server = MockServer::start();
        mock_metadata_token(&token_server);
        let servers = [MockServer::start(), MockServer::start()];
        let names = ["source-a", "source-b"];
        for (server, name) in servers.iter().zip(names) {
            server.mock(|when, then| {
                when.method(GET).path(format!(
                    "/v1/projects/{PROJECT}/secrets/secret/versions/latest:access"
                ));
                then.status(200)
                    .body(secret_body(format!("value-from-{name}").as_bytes()));
            });
        }

        let mut provider = build_provider(
            names
                .iter()
                .zip(&servers)
                .map(|(name, server)| {
                    (
                        *name,
                        source_config(Some(&server.base_url()), GcpAuth::MetadataServer),
                    )
                })
                .collect(),
        );
        for name in names {
            redirect_token_url(&mut provider, name, &token_server);
        }

        for name in names {
            assert_eq!(
                provider.get_value(&format!("{name}:secret")).unwrap(),
                format!("value-from-{name}")
            );
        }
    }

    #[rstest]
    #[case::defaults_to_metadata_server("", GcpAuth::MetadataServer)]
    #[case::service_account(
        "
    auth:
      type: service_account
      key_file: /etc/gcp/key.json",
        GcpAuth::ServiceAccount {
            key_file: PathBuf::from("/etc/gcp/key.json"),
        }
    )]
    fn test_config_deserialization(#[case] extra: &str, #[case] expected: GcpAuth) {
        let yaml = format!("sources:\n  prod:\n    project: my-project{extra}\n");
        let config: GcpSecretManagerConfig = serde_saphyr::from_str(&yaml).unwrap();
        assert_eq!(config.sources["prod"].project, PROJECT);
        assert_eq!(config.sources["prod"].endpoint, None);
        assert_eq!(config.sources["prod"].auth, expected);
    }
}
