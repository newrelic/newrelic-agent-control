use super::{SecretPath, SecretsProvider};
use crate::http::client::{HttpBuildError, HttpClient};
use crate::http::config::{HttpConfig, ProxyConfig};
use duration_str::deserialize_duration;
use http::header::InvalidHeaderValue;
use http::{HeaderValue, Request};
use serde::Deserialize;
use serde_json::Error;
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;
use url::{ParseError, Url};
use wrapper_with_default::WrapperWithDefault;

/// Default timeout for HTTP client.
const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

/// Enumerates the possible error building OpenTelemetry providers.
#[derive(Debug, Error)]
pub enum VaultError {
    #[error("could not build the vault http client: {0}")]
    HttpClient(#[from] HttpBuildError),

    #[error("could not build the vault http client: {0}")]
    InvalidHeaderValue(#[from] InvalidHeaderValue),

    #[error("could not parse mount and path for secret source: {0}")]
    ParseError(#[from] ParseError),

    #[error("could not parse mount and path for secret source: {0}")]
    SerdeError(#[from] Error),

    /// Represents an error building the HttpClient
    #[error("could not build the HTTP client: `{0}`")]
    BuildingError(String),

    #[error("http transport error: `{0}`")]
    HttpTransportError(String),

    #[error("unable to deserialize body: `{0}`")]
    DeserializeError(String),

    #[error("secret source not found")]
    SourceNotFound,

    #[error("secret not found in the specified source")]
    NotFound,
}

pub struct VaultSecretPath {
    pub source: String,
    pub mount: String,
    pub path: String,
    pub name: String,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "lowercase")] // Automatically handle lowercase conversion
pub enum SecretEngine {
    Kv1,
    Kv2,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct VaultConfig {
    pub(crate) sources: HashMap<String, VaultSourceConfig>,

    /// Client timeout
    #[serde(default)]
    pub(crate) client_timeout: ClientTimeout,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct VaultSourceConfig {
    /// Vault credentials configuration
    pub(crate) url: Url,
    pub(crate) token: String,
    pub(crate) engine: SecretEngine,
}

/// Type to represent a client timeout. It adds a default implementation to [std::time::Duration].
#[derive(Debug, Deserialize, Clone, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_CLIENT_TIMEOUT)]
pub struct ClientTimeout(#[serde(deserialize_with = "deserialize_duration")] Duration);

#[derive(Deserialize)]
struct KV1SecretData {
    data: HashMap<String, String>,
}

#[derive(Deserialize)]
struct KV2SecretData {
    data: KV2DataField,
}

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
        Self {
            url: config.url,
            token: config.token,
            engine: config.engine,
        }
    }
}

pub struct Vault {
    client: HttpClient,
    sources: HashMap<String, VaultSource>,
}

impl Vault {
    pub fn try_build(config: VaultConfig, proxy_config: ProxyConfig) -> Result<Self, VaultError> {
        let http_config = HttpConfig::new(
            config.client_timeout.clone().into(),
            config.client_timeout.into(),
            proxy_config,
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

    fn get_url_by_engine(
        mut url: Url,
        mount: &str,
        path: &str,
        engine: &SecretEngine,
    ) -> Result<Url, VaultError> {
        url = url.join(format!("{}/", mount).as_str())?;
        if engine == &SecretEngine::Kv2 {
            url = url.join("data/")?;
        }
        Ok(url.join(path)?)
    }
}

impl SecretsProvider for Vault {
    type Error = VaultError;

    fn get_secret(&self, secret_path: SecretPath) -> Result<String, Self::Error> {
        let SecretPath::Vault(vault_secret_path) = secret_path;

        let vault_source = self
            .sources
            .get(&vault_secret_path.source.to_string())
            .ok_or(VaultError::SourceNotFound)?;

        let url = Self::get_url_by_engine(
            vault_source.url.clone(),
            vault_secret_path.mount.as_str(),
            vault_secret_path.path.as_str(),
            &vault_source.engine,
        )?;

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

        let body = String::from_utf8(response.body().clone())
            .map_err(|e| VaultError::DeserializeError(format!("invalid utf8 response: {e}")))?;

        let maybe_secret = match vault_source.engine {
            SecretEngine::Kv1 => {
                let response: KV1SecretData = serde_json::from_str(&body)?;
                response.data.get(vault_secret_path.name.as_str()).cloned()
            }
            SecretEngine::Kv2 => {
                let response: KV2SecretData = serde_json::from_str(&body)?;
                response
                    .data
                    .data
                    .get(vault_secret_path.name.as_str())
                    .cloned()
            }
        };

        maybe_secret.map_or_else(
            || Err(VaultError::NotFound),
            |secret| Ok(secret.to_string()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::config::ProxyConfig;
    use crate::secrets_provider::vault::VaultConfig;
    use httpmock::Method::GET;
    use httpmock::MockServer;

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
            when.method(GET).path(KV1_PATH);
            then.status(200).body(KV1_RESPONSE);
        });
        target_server.mock(|when, then| {
            when.method(GET).path(KV2_PATH);
            then.status(200).body(KV2_RESPONSE);
        });
        let vault_config = format!(
            r#"
sources:
  sourceA:
    url: {}
    token: root
    engine: kv1
  sourceB:
    url: {}
    token: root
    engine: kv2
client_timeout: 3s
"#,
            target_server.base_url(),
            target_server.base_url()
        );
        let parsed_vault_config =
            serde_yaml::from_str::<VaultConfig>(vault_config.as_str()).unwrap();

        let vault_client = Vault::try_build(parsed_vault_config, ProxyConfig::default()).unwrap();

        struct TestCase {
            _name: &'static str,
            secret_path: SecretPath,
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
                secret_path: SecretPath::Vault(VaultSecretPath {
                    source: "sourceA".to_string(),
                    mount: "kv-v1".to_string(),
                    path: "my-secret".to_string(),
                    name: "foo1".to_string(),
                }),
                expected: Ok("bar1".to_string()),
            },
            TestCase {
                _name: "get zip1 kv1 secret",
                secret_path: SecretPath::Vault(VaultSecretPath {
                    source: "sourceA".to_string(),
                    mount: "kv-v1".to_string(),
                    path: "my-secret".to_string(),
                    name: "zip1".to_string(),
                }),
                expected: Ok("zap1".to_string()),
            },
            TestCase {
                _name: "get foo2 kv2 secret",
                secret_path: SecretPath::Vault(VaultSecretPath {
                    source: "sourceB".to_string(),
                    mount: "secret".to_string(),
                    path: "my-secret".to_string(),
                    name: "foo2".to_string(),
                }),
                expected: Ok("bar2".to_string()),
            },
            TestCase {
                _name: "get zip2 kv2 secret",
                secret_path: SecretPath::Vault(VaultSecretPath {
                    source: "sourceB".to_string(),
                    mount: "secret".to_string(),
                    path: "my-secret".to_string(),
                    name: "zip2".to_string(),
                }),
                expected: Ok("zap2".to_string()),
            },
            TestCase {
                _name: "get secret from wrong existing source returns Not Found error",
                secret_path: SecretPath::Vault(VaultSecretPath {
                    source: "sourceB".to_string(),
                    mount: "secret".to_string(),
                    path: "my-secret".to_string(),
                    name: "zip1".to_string(),
                }),
                expected: Err(VaultError::NotFound),
            },
            TestCase {
                _name: "get secret from wrong existing source returns Not Found error",
                secret_path: SecretPath::Vault(VaultSecretPath {
                    source: "sourceB".to_string(),
                    mount: "secret".to_string(),
                    path: "my-secret".to_string(),
                    name: "zip1".to_string(),
                }),
                expected: Err(VaultError::NotFound),
            },
            TestCase {
                _name: "get secret from wrong existing source returns Source Not Found error",
                secret_path: SecretPath::Vault(VaultSecretPath {
                    source: "sourceC".to_string(),
                    mount: "secret".to_string(),
                    path: "my-secret".to_string(),
                    name: "zip1".to_string(),
                }),
                expected: Err(VaultError::SourceNotFound),
            },
        ];

        for test_case in test_cases {
            test_case.run(&vault_client);
        }
    }
}
