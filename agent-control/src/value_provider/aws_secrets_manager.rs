//! Value provider that reads secrets from AWS Secrets Manager using access key credentials or an
//! EC2 instance profile.

mod imds;

use crate::http::client::{HttpBuildError, HttpClient, HttpResponseError};
use crate::http::config::{HttpConfig, ProxyConfig};
use crate::utils::sensitive_string::SensitiveString;
use crate::value_provider::ValueProvider;
use crate::value_provider::aws_secrets_manager::imds::AwsInstanceProfile;
use aws_credential_types::Credentials;
use aws_sigv4::http_request::{SignableBody, SignableRequest, SigningSettings, sign};
use aws_sigv4::sign::v4;
use duration_str::deserialize_duration;
use http::Request;
use serde::Deserialize;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use url::Url;
use wrapper_with_default::WrapperWithDefault;

const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(30);
const CONTENT_TYPE: &str = "application/x-amz-json-1.1";
const TARGET_GET_SECRET_VALUE: &str = "secretsmanager.GetSecretValue";
const RESOURCE_NOT_FOUND_ERROR: &str = "ResourceNotFoundException";
/// SigV4 service name for AWS Secrets Manager.
const SIGNING_SERVICE: &str = "secretsmanager";

/// Errors that can occur when interacting with AWS Secrets Manager.
#[derive(Debug, Error)]
pub enum AwsSecretsManagerError {
    /// The HTTP client could not be built.
    #[error("could not build the http client: {0}")]
    HttpClient(#[from] HttpBuildError),

    /// The endpoint URL could not be built or parsed.
    #[error("could not build endpoint url: {0}")]
    EndpointUrl(String),

    /// Credentials could not be retrieved from the instance metadata service.
    #[error("credentials request failed: {0}")]
    CredentialsRequest(String),

    /// The request could not be signed.
    #[error("could not sign request: {0}")]
    Signing(String),

    /// The Secrets Manager request failed with an error other than "not found".
    #[error("secret request failed: {0}")]
    SecretRequest(String),

    /// The secret does not exist.
    #[error("secret not found")]
    NotFound,

    /// The secret exists but its string value is empty.
    #[error("secret has an empty string value")]
    EmptySecret,

    /// The response contains neither a string nor a binary value.
    #[error("secret response has no value")]
    MissingValue,

    /// The secret holds binary data, only string secrets are supported.
    #[error("secret does not contain a string value")]
    NotAString,

    /// A response body could not be deserialized.
    #[error("could not deserialize response: {0}")]
    DeserializeError(String),

    /// The secret path did not match the expected `source:secret-id` format.
    #[error("secret path '{0}' does not have the expected format 'source:secret-id'")]
    IncorrectSecretPath(String),

    /// The requested source was not configured.
    #[error("secret source '{0}' not found")]
    SourceNotFound(String),
}

/// A parsed reference of the form `<source>:<secret-id>`, where the secret id is a name or an ARN.
struct AwsSecretPath {
    source: String,
    secret_id: String,
}

impl FromStr for AwsSecretPath {
    type Err = AwsSecretsManagerError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Only the first colon separates the source, so ARNs are valid secret ids.
        let (source, secret_id) = s
            .split_once(':')
            .filter(|(a, b)| !a.is_empty() && !b.is_empty())
            .ok_or_else(|| AwsSecretsManagerError::IncorrectSecretPath(s.to_string()))?;
        Ok(Self {
            source: source.to_string(),
            secret_id: secret_id.to_string(),
        })
    }
}

/// Client timeout with a sensible default.
#[derive(Debug, Deserialize, Clone, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_CLIENT_TIMEOUT)]
pub struct ClientTimeout(#[serde(deserialize_with = "deserialize_duration")] Duration);

/// Authentication method for an AWS Secrets Manager source.
#[derive(Debug, Default, Deserialize, PartialEq, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AwsAuth {
    /// Credentials of the IAM role attached to the EC2 instance, retrieved from IMDSv2.
    /// Works only from EC2 instances with an instance profile.
    #[default]
    InstanceProfile,

    /// Long-lived IAM user access key credentials. Works from any environment. They are read once from the
    /// configuration, so temporary credentials are not supported as they would expire.
    AccessKey {
        /// AWS access key id.
        access_key_id: String,
        /// AWS secret access key.
        secret_access_key: SensitiveString,
    },
}

/// Configuration for a single AWS Secrets Manager source.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct AwsSecretsManagerSourceConfig {
    /// AWS region of the secrets (e.g. `us-east-1`).
    pub region: String,

    /// Overrides the default `https://secretsmanager.<region>.amazonaws.com/` endpoint,
    /// for example to use a VPC endpoint.
    #[serde(default)]
    pub endpoint: Option<Url>,

    /// Authentication method. Defaults to `instance_profile`.
    #[serde(default)]
    pub auth: AwsAuth,
}

/// Configuration for the AWS Secrets Manager provider: a set of named sources and HTTP client settings.
#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct AwsSecretsManagerConfig {
    /// Named AWS Secrets Manager sources.
    pub sources: HashMap<String, AwsSecretsManagerSourceConfig>,

    /// Timeout applied to both connect and read phases of every HTTP request.
    #[serde(default)]
    pub(crate) client_timeout: ClientTimeout,

    /// Injected at runtime by the caller; not read from config.
    #[serde(skip)]
    pub proxy_config: ProxyConfig,
}

/// Ready-to-use representation of one configured auth method.
enum AwsAuthRuntime {
    InstanceProfile(imds::AwsInstanceProfile),
    AccessKey(Credentials),
}

impl AwsAuthRuntime {
    /// Returns valid credentials, refreshing the instance profile ones when they are about to expire.
    fn credentials(&self) -> Result<Credentials, AwsSecretsManagerError> {
        match self {
            Self::AccessKey(credentials) => Ok(credentials.clone()),
            Self::InstanceProfile(profile) => profile.credentials(),
        }
    }
}

/// Runtime representation of a single named source.
struct AwsSecretsManagerSource {
    region: String,
    endpoint: Url,
    auth: AwsAuthRuntime,
}

/// `GetSecretValue` response shape (only the fields we need).
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GetSecretValueResponse {
    secret_string: Option<String>,
    secret_binary: Option<String>,
}

/// AWS JSON protocol error shape.
#[derive(Deserialize)]
struct AwsErrorResponse {
    #[serde(rename = "__type")]
    error_type: Option<String>,
}

/// AWS Secrets Manager provider. Holds a shared HTTP client and one runtime source per configured name.
// NOTE: `aws-sdk-secretsmanager` was discarded because its async HTTP client has its own proxy
// handling, which behaves differently from the HTTP client used by the rest of Agent Control.
// Adopting it would require extra research to align the proxy behavior (e.g. a custom HTTP client
// implementation for the SDK). Reconsider it if more authentication methods are needed, since the
// SDK's credential provider chain supports them out of the box.
pub struct AwsSecretsManager {
    client: HttpClient,
    sources: HashMap<String, AwsSecretsManagerSource>,
}

impl AwsSecretsManager {
    /// Builds an `AwsSecretsManager` from the given configuration.
    pub fn try_build(config: AwsSecretsManagerConfig) -> Result<Self, AwsSecretsManagerError> {
        let timeout: Duration = config.client_timeout.into();
        let http_config = HttpConfig::new(timeout, timeout, config.proxy_config);

        let sources = config
            .sources
            .into_iter()
            .map(|(name, source_config)| {
                let source = AwsSecretsManagerSource::try_new(source_config, timeout)?;
                Ok((name, source))
            })
            .collect::<Result<HashMap<_, _>, AwsSecretsManagerError>>()?;

        Ok(Self {
            client: HttpClient::new(http_config)?,
            sources,
        })
    }

    /// Builds the signed `GetSecretValue` request.
    fn signed_request(
        source: &AwsSecretsManagerSource,
        credentials: Credentials,
        secret_id: &str,
    ) -> Result<Request<Vec<u8>>, AwsSecretsManagerError> {
        let body = serde_json::json!({ "SecretId": secret_id })
            .to_string()
            .into_bytes();

        let identity = credentials.into();
        let params = v4::SigningParams::builder()
            .identity(&identity)
            .region(&source.region)
            .name(SIGNING_SERVICE)
            .time(SystemTime::now())
            .settings(SigningSettings::default())
            .build()
            .map_err(|e| AwsSecretsManagerError::Signing(e.to_string()))?
            .into();

        let signable = SignableRequest::new(
            "POST",
            source.endpoint.as_str(),
            [
                ("content-type", CONTENT_TYPE),
                ("x-amz-target", TARGET_GET_SECRET_VALUE),
            ]
            .into_iter(),
            SignableBody::Bytes(&body),
        )
        .map_err(|e| AwsSecretsManagerError::Signing(e.to_string()))?;

        let (instructions, _signature) = sign(signable, &params)
            .map_err(|e| AwsSecretsManagerError::Signing(e.to_string()))?
            .into_parts();

        let mut request = Request::builder()
            .method("POST")
            .uri(source.endpoint.as_str())
            .header("content-type", CONTENT_TYPE)
            .header("x-amz-target", TARGET_GET_SECRET_VALUE)
            .body(body)
            .map_err(|e| AwsSecretsManagerError::SecretRequest(e.to_string()))?;
        instructions.apply_to_request_http1x(&mut request);
        Ok(request)
    }
}

impl AwsSecretsManagerSource {
    fn try_new(
        config: AwsSecretsManagerSourceConfig,
        timeout: Duration,
    ) -> Result<Self, AwsSecretsManagerError> {
        let endpoint = match config.endpoint {
            Some(endpoint) => endpoint,
            None => Url::parse(&format!(
                "https://secretsmanager.{}.amazonaws.com/",
                config.region
            ))
            .map_err(|e| AwsSecretsManagerError::EndpointUrl(e.to_string()))?,
        };

        let auth = match config.auth {
            AwsAuth::InstanceProfile => {
                AwsAuthRuntime::InstanceProfile(AwsInstanceProfile::try_new(timeout)?)
            }
            AwsAuth::AccessKey {
                access_key_id,
                secret_access_key,
            } => AwsAuthRuntime::AccessKey(Credentials::new(
                access_key_id,
                secret_access_key.expose_secret(),
                None,
                None,
                "agent-control-access-key",
            )),
        };

        Ok(Self {
            region: config.region,
            endpoint,
            auth,
        })
    }
}

impl ValueProvider for AwsSecretsManager {
    type Error = AwsSecretsManagerError;

    fn get_value(&self, secret_path: &str) -> Result<String, Self::Error> {
        let AwsSecretPath {
            source: source_name,
            secret_id,
        } = AwsSecretPath::from_str(secret_path)?;

        let source = self
            .sources
            .get(&source_name)
            .ok_or_else(|| AwsSecretsManagerError::SourceNotFound(source_name.clone()))?;

        let credentials = source.auth.credentials()?;
        let request = Self::signed_request(source, credentials, &secret_id)?;

        let response = self.client.send(request).map_err(|e| match e {
            HttpResponseError::UnsuccessfulResponse { status_code, body } => {
                if is_resource_not_found(&body) {
                    AwsSecretsManagerError::NotFound
                } else {
                    AwsSecretsManagerError::SecretRequest(format!(
                        "secrets manager responded with status {status_code}: {}",
                        String::from_utf8_lossy(&body)
                    ))
                }
            }
            _ => AwsSecretsManagerError::SecretRequest(e.to_string()),
        })?;

        let body = String::from_utf8(response.into_body())
            .map_err(|e| AwsSecretsManagerError::DeserializeError(format!("invalid utf8: {e}")))?;

        let secret: GetSecretValueResponse = serde_json::from_str(&body)
            .map_err(|e| AwsSecretsManagerError::DeserializeError(e.to_string()))?;

        match (secret.secret_string, secret.secret_binary) {
            (Some(value), _) if !value.is_empty() => Ok(value),
            (Some(_), _) => Err(AwsSecretsManagerError::EmptySecret),
            (None, Some(_)) => Err(AwsSecretsManagerError::NotAString),
            (None, None) => Err(AwsSecretsManagerError::MissingValue),
        }
    }
}

/// Whether an error response body reports a missing secret.
fn is_resource_not_found(body: &[u8]) -> bool {
    serde_json::from_slice::<AwsErrorResponse>(body)
        .ok()
        .and_then(|e| e.error_type)
        .is_some_and(|t| t.contains(RESOURCE_NOT_FOUND_ERROR))
}

#[cfg(test)]
#[allow(missing_docs)]
pub mod tests {
    use super::imds::tests::{ACCESS_KEY, SECRET_KEY, mock_imds};
    use super::*;
    use assert_matches::assert_matches;
    use httpmock::Method::POST;
    use httpmock::MockServer;
    use mockall::mock;

    mock! {
        pub AwsSecretsManager {}

        impl ValueProvider for AwsSecretsManager {
            type Error = AwsSecretsManagerError;

            fn get_value(&self, path: &str) -> Result<String, AwsSecretsManagerError>;
        }
    }

    const SOURCE_NAME: &str = "test-source";
    const REGION: &str = "us-east-1";
    const AUTH_HEADER_PATTERN: &str = r"^AWS4-HMAC-SHA256 Credential=AKIDTEST/\d{8}/us-east-1/secretsmanager/aws4_request, SignedHeaders=.*content-type;host;.*x-amz-target.*, Signature=[0-9a-f]{64}$";

    fn access_key_auth() -> AwsAuth {
        AwsAuth::AccessKey {
            access_key_id: ACCESS_KEY.to_string(),
            secret_access_key: SECRET_KEY.into(),
        }
    }

    fn build_provider(secrets_server: &MockServer, auth: AwsAuth) -> AwsSecretsManager {
        let config = AwsSecretsManagerConfig {
            sources: HashMap::from([(
                SOURCE_NAME.to_string(),
                AwsSecretsManagerSourceConfig {
                    region: REGION.to_string(),
                    endpoint: Some(Url::parse(&secrets_server.base_url()).unwrap()),
                    auth,
                },
            )]),
            ..Default::default()
        };
        AwsSecretsManager::try_build(config).unwrap()
    }

    /// Builds a provider whose instance profile credentials come from the given IMDS mock server.
    fn build_instance_profile_provider(
        secrets_server: &MockServer,
        imds_server: &MockServer,
        proxy_config: ProxyConfig,
    ) -> AwsSecretsManager {
        let profile =
            AwsInstanceProfile::with_base_url(Duration::from_secs(5), imds_server.base_url())
                .unwrap();
        let source = AwsSecretsManagerSource {
            region: REGION.to_string(),
            endpoint: Url::parse(&secrets_server.base_url()).unwrap(),
            auth: AwsAuthRuntime::InstanceProfile(profile),
        };
        let http_config =
            HttpConfig::new(Duration::from_secs(5), Duration::from_secs(5), proxy_config);
        AwsSecretsManager {
            client: HttpClient::new(http_config).unwrap(),
            sources: HashMap::from([(SOURCE_NAME.to_string(), source)]),
        }
    }

    fn secret_path(secret_id: &str) -> String {
        format!("{SOURCE_NAME}:{secret_id}")
    }

    #[test]
    fn test_access_key_get_secret_success() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/")
                .header("x-amz-target", TARGET_GET_SECRET_VALUE)
                .header("content-type", CONTENT_TYPE)
                .header_matches("authorization", AUTH_HEADER_PATTERN)
                .header_exists("x-amz-date")
                .json_body(serde_json::json!({"SecretId": "my/secret"}));
            then.status(200)
                .body(r#"{"SecretString":"super-secret-value"}"#);
        });

        let provider = build_provider(&server, access_key_auth());
        assert_eq!(
            provider.get_value(&secret_path("my/secret")).unwrap(),
            "super-secret-value"
        );
        mock.assert();
    }

    #[test]
    fn test_secret_id_can_be_an_arn() {
        let server = MockServer::start();
        let arn = "arn:aws:secretsmanager:us-east-1:123456789012:secret:my-secret-AbCdEf";
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/")
                .json_body(serde_json::json!({ "SecretId": arn }));
            then.status(200).body(r#"{"SecretString":"val"}"#);
        });

        let provider = build_provider(&server, access_key_auth());
        assert_eq!(provider.get_value(&secret_path(arn)).unwrap(), "val");
        mock.assert();
    }

    #[test]
    fn test_secret_not_found() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/");
            then.status(400).body(
                r#"{"__type":"ResourceNotFoundException","Message":"Secrets Manager can't find the specified secret."}"#,
            );
        });

        let provider = build_provider(&server, access_key_auth());
        assert_matches!(
            provider.get_value(&secret_path("missing")),
            Err(AwsSecretsManagerError::NotFound)
        );
    }

    #[test]
    fn test_access_denied() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/");
            then.status(400)
                .body(r#"{"__type":"AccessDeniedException","Message":"denied"}"#);
        });

        let provider = build_provider(&server, access_key_auth());
        assert_matches!(
            provider.get_value(&secret_path("my-secret")),
            Err(AwsSecretsManagerError::SecretRequest(_))
        );
    }

    #[test]
    fn test_secret_value_empty() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/");
            then.status(200).body(r#"{"SecretString":""}"#);
        });

        let provider = build_provider(&server, access_key_auth());
        assert_matches!(
            provider.get_value(&secret_path("my-secret")),
            Err(AwsSecretsManagerError::EmptySecret)
        );
    }

    #[test]
    fn test_response_without_value() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/");
            then.status(200).body("{}");
        });

        let provider = build_provider(&server, access_key_auth());
        assert_matches!(
            provider.get_value(&secret_path("my-secret")),
            Err(AwsSecretsManagerError::MissingValue)
        );
    }

    #[test]
    fn test_binary_secret_is_rejected() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/");
            then.status(200).body(r#"{"SecretBinary":"AAEC"}"#);
        });

        let provider = build_provider(&server, access_key_auth());
        assert_matches!(
            provider.get_value(&secret_path("my-secret")),
            Err(AwsSecretsManagerError::NotAString)
        );
    }

    #[test]
    fn test_invalid_json_response() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/");
            then.status(200).body("not json at all");
        });

        let provider = build_provider(&server, access_key_auth());
        assert_matches!(
            provider.get_value(&secret_path("my-secret")),
            Err(AwsSecretsManagerError::DeserializeError(_))
        );
    }

    #[test]
    fn test_connection_failed() {
        let config = AwsSecretsManagerConfig {
            sources: HashMap::from([(
                SOURCE_NAME.to_string(),
                AwsSecretsManagerSourceConfig {
                    region: REGION.to_string(),
                    endpoint: Some(Url::parse("http://127.0.0.1:1").unwrap()),
                    auth: access_key_auth(),
                },
            )]),
            ..Default::default()
        };
        let provider = AwsSecretsManager::try_build(config).unwrap();

        assert_matches!(
            provider.get_value(&secret_path("my-secret")),
            Err(AwsSecretsManagerError::SecretRequest(_))
        );
    }

    #[test]
    fn test_default_endpoint_is_regional() {
        let source = AwsSecretsManagerSource::try_new(
            AwsSecretsManagerSourceConfig {
                region: "eu-west-1".to_string(),
                endpoint: None,
                auth: access_key_auth(),
            },
            DEFAULT_CLIENT_TIMEOUT,
        )
        .unwrap();
        assert_eq!(
            source.endpoint.as_str(),
            "https://secretsmanager.eu-west-1.amazonaws.com/"
        );
    }

    #[test]
    fn test_instance_profile_get_secret_success() {
        let secrets_server = MockServer::start();
        let imds_server = MockServer::start();
        mock_imds(&imds_server, "2999-01-01T00:00:00Z");

        let mock = secrets_server.mock(|when, then| {
            when.method(POST)
                .path("/")
                .header("x-amz-security-token", "session-token")
                .header_matches("authorization", AUTH_HEADER_PATTERN);
            then.status(200).body(r#"{"SecretString":"hunter2"}"#);
        });

        let provider =
            build_instance_profile_provider(&secrets_server, &imds_server, ProxyConfig::default());
        assert_eq!(
            provider.get_value(&secret_path("db-password")).unwrap(),
            "hunter2"
        );
        mock.assert();
    }

    #[test]
    fn test_instance_profile_imds_bypasses_proxy() {
        let secrets_server = MockServer::start();
        let imds_server = MockServer::start();
        let token_mock = mock_imds(&imds_server, "2999-01-01T00:00:00Z");
        let secrets_mock = secrets_server.mock(|when, then| {
            when.method(POST)
                .path("/")
                .header("x-amz-security-token", "session-token");
            then.status(200).body(r#"{"SecretString":"hunter2"}"#);
        });

        // The proxy only forwards to the secrets server, so an IMDS request sent through it would fail.
        let proxy_server = MockServer::start();
        proxy_server.proxy(|rule| {
            rule.filter(|when| {
                when.host(secrets_server.host()).port(secrets_server.port());
            });
        });

        let provider = build_instance_profile_provider(
            &secrets_server,
            &imds_server,
            ProxyConfig::from_url(proxy_server.base_url()),
        );
        assert_eq!(
            provider.get_value(&secret_path("db-password")).unwrap(),
            "hunter2"
        );
        token_mock.assert_calls(1);
        secrets_mock.assert();
    }

    #[test]
    fn test_instance_profile_credentials_are_cached() {
        let secrets_server = MockServer::start();
        let imds_server = MockServer::start();
        let token_mock = mock_imds(&imds_server, "2999-01-01T00:00:00Z");
        secrets_server.mock(|when, then| {
            when.method(POST).path("/");
            then.status(200).body(r#"{"SecretString":"val"}"#);
        });

        let provider =
            build_instance_profile_provider(&secrets_server, &imds_server, ProxyConfig::default());
        provider.get_value(&secret_path("a")).unwrap();
        provider.get_value(&secret_path("b")).unwrap();

        token_mock.assert_calls(1);
    }

    #[test]
    fn test_instance_profile_expiring_credentials_are_refreshed() {
        let secrets_server = MockServer::start();
        let imds_server = MockServer::start();
        let token_mock = mock_imds(&imds_server, "2000-01-01T00:00:00Z");
        secrets_server.mock(|when, then| {
            when.method(POST).path("/");
            then.status(200).body(r#"{"SecretString":"val"}"#);
        });

        let provider =
            build_instance_profile_provider(&secrets_server, &imds_server, ProxyConfig::default());
        provider.get_value(&secret_path("a")).unwrap();
        provider.get_value(&secret_path("b")).unwrap();

        token_mock.assert_calls(2);
    }

    #[test]
    fn test_incorrect_secret_path() {
        let server = MockServer::start();
        let provider = build_provider(&server, access_key_auth());

        for path in ["no-colon-here", ":secret", "source:"] {
            assert_matches!(
                provider.get_value(path),
                Err(AwsSecretsManagerError::IncorrectSecretPath(_)),
                "path: {path}"
            );
        }
    }

    #[test]
    fn test_source_not_found() {
        let server = MockServer::start();
        let provider = build_provider(&server, access_key_auth());

        assert_matches!(
            provider.get_value("nonexistent-source:my-secret"),
            Err(AwsSecretsManagerError::SourceNotFound(name)) if name == "nonexistent-source"
        );
    }

    #[test]
    fn test_multiple_sources() {
        let server_a = MockServer::start();
        let server_b = MockServer::start();
        server_a.mock(|when, then| {
            when.method(POST).path("/");
            then.status(200).body(r#"{"SecretString":"value-from-a"}"#);
        });
        server_b.mock(|when, then| {
            when.method(POST).path("/");
            then.status(200).body(r#"{"SecretString":"value-from-b"}"#);
        });

        let source = |server: &MockServer| AwsSecretsManagerSourceConfig {
            region: REGION.to_string(),
            endpoint: Some(Url::parse(&server.base_url()).unwrap()),
            auth: access_key_auth(),
        };
        let config = AwsSecretsManagerConfig {
            sources: HashMap::from([
                ("source-a".to_string(), source(&server_a)),
                ("source-b".to_string(), source(&server_b)),
            ]),
            ..Default::default()
        };
        let provider = AwsSecretsManager::try_build(config).unwrap();

        assert_eq!(
            provider.get_value("source-a:secret").unwrap(),
            "value-from-a"
        );
        assert_eq!(
            provider.get_value("source-b:secret").unwrap(),
            "value-from-b"
        );
    }

    #[test]
    fn test_config_deserialization_defaults_to_instance_profile() {
        let config: AwsSecretsManagerConfig = serde_saphyr::from_str(
            r#"
sources:
  prod:
    region: us-east-1
"#,
        )
        .unwrap();
        assert_eq!(config.sources["prod"].auth, AwsAuth::InstanceProfile);
        assert_eq!(config.sources["prod"].endpoint, None);
    }

    #[test]
    fn test_config_deserialization_access_key() {
        let config: AwsSecretsManagerConfig = serde_saphyr::from_str(
            r#"
sources:
  prod:
    region: us-east-1
    auth:
      type: access_key
      access_key_id: AKID
      secret_access_key: secret
"#,
        )
        .unwrap();
        assert_eq!(
            config.sources["prod"].auth,
            AwsAuth::AccessKey {
                access_key_id: "AKID".to_string(),
                secret_access_key: "secret".into(),
            }
        );
    }
}
