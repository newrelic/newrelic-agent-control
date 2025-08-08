use super::SecretsProvider;
use crate::http::client::{HttpBuildError, HttpClient};
use crate::http::config::{HttpConfig, ProxyConfig};
use duration_str::deserialize_duration;
use http::header::InvalidHeaderValue;
use http::{HeaderValue, Request};
use serde::Deserialize;
use serde_json::Error;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;
use thiserror::Error;
use url::{ParseError, Url};
use wrapper_with_default::WrapperWithDefault;

/// Default timeout for HTTP client.
const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

/// Enumerates the possible errors that can occur when interacting with Vault.
#[derive(Debug, Error)]
pub enum VaultError {
    #[error("could not build the vault http client: {0}")]
    HttpClient(#[from] HttpBuildError),

    #[error("invalid header: {0}")]
    InvalidHeaderValue(#[from] InvalidHeaderValue),

    #[error("could not parse mount and path for secret source: {0}")]
    ParseError(#[from] ParseError),

    #[error("error deserializing the config: {0}")]
    SerdeError(#[from] Error),

    /// Represents an error building the HttpClient
    #[error("could not build the HTTP client: `{0}`")]
    BuildingError(String),

    #[error("http transport error: `{0}`")]
    HttpTransportError(String),

    #[error("unable to deserialize body: `{0}`")]
    DeserializeError(String),

    #[error("secret path '{0}' does not have a valid format 'source:mount:path:name'")]
    IncorrectSecretPath(String),

    #[error("secret source not found")]
    SourceNotFound,

    #[error("secret not found in the specified source")]
    NotFound,

    #[error("{0}")]
    GenericError(String),
}

/// Represents a path to a secret in Vault, including source, mount, path, and name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultSecretPath {
    pub source: String,
    pub mount: String,
    pub path: String,
    pub name: String,
}

impl FromStr for VaultSecretPath {
    type Err = VaultError;

    fn from_str(secret_path: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = secret_path.split(':').collect();
        if parts.len() != 4 || parts.iter().any(|p| p.is_empty()) {
            return Err(VaultError::IncorrectSecretPath(secret_path.to_string()));
        }

        let secret_path = VaultSecretPath {
            source: parts[0].to_string(),
            mount: parts[1].to_string(),
            path: parts[2].to_string(),
            name: parts[3].to_string(),
        };

        Ok(secret_path)
    }
}

/// Represents HashiCorp Vault secret engines for key-value storage.
/// Kv1 is the original version, while Kv2 adds versioning capabilities.
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "lowercase")] // Automatically handle lowercase conversion
pub enum SecretEngine {
    Kv1,
    Kv2,
}

impl SecretEngine {
    fn get_url(&self, url: Url, mount: &str, path: &str) -> Result<Url, VaultError> {
        match self {
            SecretEngine::Kv1 => Ok(url.join(format!("{mount}/{path}").as_str())?),
            SecretEngine::Kv2 => Ok(url.join(format!("{mount}/data/{path}").as_str())?),
        }
    }

    fn parse_secret_response(
        &self,
        name: &str,
        body: String,
    ) -> Result<Option<String>, VaultError> {
        Ok(match self {
            SecretEngine::Kv1 => {
                let response: KV1SecretData = serde_json::from_str(&body)?;
                response.data.get(name).cloned()
            }
            SecretEngine::Kv2 => {
                let response: KV2SecretData = serde_json::from_str(&body)?;
                response.data.data.get(name).cloned()
            }
        })
    }
}

/// Configuration for a Vault source, including URL, token, and engine type.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct VaultSourceConfig {
    url: Url,
    token: String,
    engine: SecretEngine,
}

/// Type to represent a client timeout. It adds a default implementation to [std::time::Duration].
#[derive(Debug, Deserialize, Clone, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_CLIENT_TIMEOUT)]
pub struct ClientTimeout(#[serde(deserialize_with = "deserialize_duration")] Duration);

/// Represents the data structure for KV1 secrets. Used for deserialization.
#[derive(Deserialize)]
struct KV1SecretData {
    data: HashMap<String, String>,
}

/// Represents the data structure for KV2 secrets. Used for deserialization.
#[derive(Deserialize)]
struct KV2SecretData {
    data: KV2DataField,
}

/// Represents the inner data field for KV2 secrets. Used for deserialization.
#[derive(Deserialize)]
struct KV2DataField {
    data: HashMap<String, String>,
}

pub struct VaultSource {
    url: Url,
    token: String,
    engine: SecretEngine,
}

impl VaultSource {
    fn new(config: VaultSourceConfig) -> Self {
        let mut url = config.url;
        let path = url.path();
        if !path.ends_with('/') {
            url.set_path(&format!("{path}/"));
        }

        Self {
            url,
            token: config.token,
            engine: config.engine,
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct VaultConfig {
    pub(crate) sources: HashMap<String, VaultSourceConfig>,

    /// Client timeout
    #[serde(default)]
    pub(crate) client_timeout: ClientTimeout,

    #[serde(skip)]
    pub proxy_config: ProxyConfig,
}

/// Represents a Vault client, including HTTP client and configured sources.
pub struct Vault {
    client: HttpClient,
    sources: HashMap<String, VaultSource>,
}

impl Vault {
    /// Attempts to build a Vault instance from the given configuration.
    pub fn try_build(config: VaultConfig) -> Result<Self, VaultError> {
        let http_config = HttpConfig::new(
            config.client_timeout.clone().into(),
            config.client_timeout.into(),
            config.proxy_config,
        );

        let sources = config
            .sources
            .iter()
            .map(|(source_name, source_config)| {
                let source = VaultSource::new(source_config.clone());
                Ok((source_name.clone(), source))
            })
            .collect::<Result<HashMap<String, VaultSource>, VaultError>>()?;

        Ok(Self {
            client: HttpClient::new(http_config).map_err(VaultError::HttpClient)?,
            sources,
        })
    }
}

/// Implements the SecretsProvider trait for Vault, allowing it to retrieve secrets.
impl SecretsProvider for Vault {
    type Error = VaultError;

    fn get_secret(&self, secret_path: &str) -> Result<String, Self::Error> {
        let VaultSecretPath {
            source,
            mount,
            path,
            name,
        } = VaultSecretPath::from_str(secret_path)?;

        let vault_source = self
            .sources
            .get(&source)
            .ok_or(VaultError::SourceNotFound)?;

        let url =
            vault_source
                .engine
                .get_url(vault_source.url.clone(), mount.as_str(), path.as_str())?;

        let mut request = Request::builder()
            .method("GET")
            .uri(url.as_str())
            .body(Vec::new())
            .map_err(|e| VaultError::BuildingError(e.to_string()))?;
        request.headers_mut().insert(
            "X-Vault-Token",
            HeaderValue::from_str(vault_source.token.as_str())?,
        );

        let response = self
            .client
            .send(request)
            .map_err(|e| VaultError::HttpTransportError(e.to_string()))?;

        let body = String::from_utf8(response.into_body())
            .map_err(|e| VaultError::DeserializeError(format!("invalid utf8 response: {e}")))?;

        let maybe_secret = vault_source.engine.parse_secret_response(&name, body)?;

        maybe_secret.map_or_else(
            || Err(VaultError::NotFound),
            |secret| Ok(secret.to_string()),
        )
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::secrets_provider::vault::VaultConfig;
    use httpmock::Method::GET;
    use httpmock::MockServer;
    use mockall::mock;

    mock! {
        pub Vault {}

        impl SecretsProvider for Vault {
            type Error = VaultError;

            fn get_secret(&self, secret_path: &str) -> Result<String, VaultError>;
        }
    }

    const KV1_PATH: &str = "/kv-v1/my-secret";
    const KV1_RESPONSE: &str = r#"{
        "request_id": "b42f4663-0368-fae6-fbb4-d198def4fba6",
        "data": {
            "foo1": "bar1",
            "zip1": "zap1"
        },
        "mount_type": "kv"
    }"#;

    const KV2_PATH: &str = "/secret/data/my-secret";
    const KV2_RESPONSE: &str = r#"{
        "request_id": "1257f4cf-f206-51e6-4c99-2e2031351adb",
        "data": {
            "data": {
                "foo2": "bar2",
                "zip2": "zap2"
            },
            "metadata": {
                "created_time": "2025-07-14T09:50:32.382623752Z",
                "custom_metadata": null,
                "deletion_time": "",
                "destroyed": false,
                "version": 1
            }
        },
        "mount_type": "kv"
    }"#;

    #[test]
    fn test_get_secrets() {
        let target_server = MockServer::start();
        target_server.mock(|when, then| {
            when.method(GET).path(format!("/v1{}", KV1_PATH));
            then.status(200).body(KV1_RESPONSE);
        });
        target_server.mock(|when, then| {
            when.method(GET).path(format!("/v1{}", KV2_PATH));
            then.status(200).body(KV2_RESPONSE);
        });

        // We set one source with and another without trailing-slash to ensure correct creation
        // by the VaultSource creator
        let vault_config = format!(
            r#"
sources:
  sourceA:
    url: {}/v1
    token: root
    engine: kv1
  sourceB:
    url: {}/v1/
    token: root
    engine: kv2
client_timeout: 3s
"#,
            target_server.base_url(),
            target_server.base_url()
        );
        let parsed_vault_config =
            serde_yaml::from_str::<VaultConfig>(vault_config.as_str()).unwrap();

        let vault_client = Vault::try_build(parsed_vault_config).unwrap();

        struct TestCase {
            _name: &'static str,
            secret_path: &'static str,
            expected: Result<String, VaultError>,
        }

        impl TestCase {
            fn run(self, vault: &Vault) {
                let actual = vault.get_secret(self.secret_path);
                if self.expected.is_ok() {
                    assert_eq!(
                        self.expected.unwrap(),
                        actual.unwrap_or("".to_string()),
                        "Test Name: {}",
                        self._name
                    );
                } else {
                    assert!(actual.is_err());
                    assert_eq!(
                        self.expected.unwrap_err().to_string(),
                        actual.unwrap_err().to_string()
                    );
                }
            }
        }

        let test_cases = vec![
            TestCase {
                _name: "get foo1 kv1 secret",
                secret_path: "sourceA:kv-v1:my-secret:foo1",
                expected: Ok("bar1".to_string()),
            },
            TestCase {
                _name: "get zip1 kv1 secret",
                secret_path: "sourceA:kv-v1:my-secret:zip1",
                expected: Ok("zap1".to_string()),
            },
            TestCase {
                _name: "get foo2 kv2 secret",
                secret_path: "sourceB:secret:my-secret:foo2",
                expected: Ok("bar2".to_string()),
            },
            TestCase {
                _name: "get zip2 kv2 secret",
                secret_path: "sourceB:secret:my-secret:zip2",
                expected: Ok("zap2".to_string()),
            },
            TestCase {
                _name: "get secret from wrong existing source returns Not Found error",
                secret_path: "sourceB:secret:my-secret:zip1",
                expected: Err(VaultError::NotFound),
            },
            TestCase {
                _name: "get secret from wrong existing source returns Source Not Found error",
                secret_path: "sourceC:secret:my-secret:zip1",
                expected: Err(VaultError::SourceNotFound),
            },
        ];

        for test_case in test_cases {
            test_case.run(&vault_client);
        }
    }
}
