use std::collections::HashMap;
use std::time::Duration;
use futures::TryFutureExt;
use http::{HeaderMap, HeaderName, HeaderValue, Request};
use serde::Deserialize;
use serde_json::Error;
use thiserror::Error;
use url::{ParseError, Url};
use wrapper_with_default::WrapperWithDefault;
use crate::http::client::{HttpBuildError, HttpClient};
use crate::http::config::{HttpConfig, ProxyConfig};
use super::SecretsProvidersError;
use duration_str::deserialize_duration;
use http::header::InvalidHeaderValue;

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
    engine: SecretEngine
}

impl VaultSource {
    fn new(config: VaultSourceConfig) -> Self {
        Self { url: config.url, token: config.token, engine: config.engine }
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

    pub fn get_secret(&self, source: &str, mount: &str, path: &str, name: &str) -> Result<String, VaultError> {
        if let Some(vault_source) = self.sources.get(&source.to_string()) {
            let url = Self::get_url_by_engine(vault_source.url.clone(), mount, path, &vault_source.engine)?;

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

            let body: String = String::from_utf8(response.body().clone()).map_err(|e| {
                VaultError::DeserializeError(format!("invalid utf8 response: {e}"))
            })?;

            let maybe_secret = match vault_source.engine {
                SecretEngine::Kv1 => {
                    serde_json::from_str::<KV1SecretData>(body.as_str())
                        .map(|response| response.data.get(name).cloned())?
                }
                SecretEngine::Kv2 => {
                    serde_json::from_str::<KV2SecretData>(body.as_str())
                        .map(|response| response.data.data.get(name).cloned())?
                }
            };

            match maybe_secret {
                Some(secret) => {
                    return Ok(secret.to_string());
                }
                None => {
                    return Err(VaultError::NotFound);
                }
            }
        }
        Err(VaultError::SourceNotFound)
    }

    fn get_url_by_engine(mut url: Url, mount: &str, path: &str, engine: &SecretEngine) -> Result<Url, VaultError> {
        url = url.join(format!("{}/", mount).as_str())?;
        if engine == &SecretEngine::Kv2 {
            url = url.join("data/")?;
        }
        Ok(url.join(path)?)
    }
}
