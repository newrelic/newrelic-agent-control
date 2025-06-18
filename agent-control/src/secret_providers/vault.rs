use std::collections::HashMap;
use serde::Deserialize;
use thiserror::Error;
use tokio::runtime::Runtime;
use tracing::debug;
use url::Url;
use vaultrs::client::{VaultClient, VaultClientSettingsBuilder, VaultClientSettingsBuilderError};
use vaultrs::error::ClientError;
use vaultrs::kv2;


/// Enumerates the possible error building OpenTelemetry providers.
#[derive(Debug, Error)]
pub enum VaultBuildError {
    #[error("could not build the vault client: {0}")]
    VaultClientSettingsBuilderError(#[from] VaultClientSettingsBuilderError),

    #[error("could not build the vault client: {0}")]
    ClientError(#[from] ClientError),
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct VaultConfig {
    #[serde(flatten)]
    pub(crate) sources: HashMap<String, VaultSourceConfig>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct VaultSourceConfig {
    /// Vault credentials configuration
    pub(crate) url: Url,
    pub(crate) token: String,
}

#[derive(Debug, Deserialize)]
struct SecretData {
    #[serde(flatten)]
    data: HashMap<String, String>, // Ensure proper struct representation
}

pub struct Vault {
    sources: HashMap<String, VaultSource>,
}

impl Vault {
    pub fn try_build(config: VaultConfig) -> Result<Self, VaultBuildError> {
        let sources = config
            .sources
            .iter()
            .map(|(k, v)| {
                VaultSource::try_build(v.clone()).map(|source| (k.clone(), source))
            })
            .collect::<Result<HashMap<String, VaultSource>, VaultBuildError>>()?;

        Ok(Self { sources })
    }

    pub fn get_secret(&self, source: &str, mount: &str, path: &str, name: &str) -> Option<String> {
        if let Some(vault_source) = self.sources.get(&source.to_string()) {
            return vault_source.get_secret(mount, path, name);
        }
        None
    }
}

pub struct VaultSource {
    client: VaultClient,
}

impl VaultSource {
    fn try_build(config: VaultSourceConfig) -> Result<Self, VaultBuildError> {
        // Create the Vault client
        let client = VaultClient::new(
            VaultClientSettingsBuilder::default()
                .address(config.url)
                .token(config.token)
                .build()?,
        )?;

        Ok(
            Self {
                client
            }
        )
    }

    pub(crate) fn get_secret(&self, mount: &str, path: &str, name: &str) -> Option<String> {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            // Attempt to retrieve the secret using KV version 2 (for KV1 uses it's own type)
            // TODO there can be a cache for mount/path secret we try to retrieve before doing this
            //  read this is due to parsing, for example:
            //  - ${nr-vault:sourceA:secret:my-secret:username}
            //  - ${nr-vault:sourceA:secret:my-secret:password}
            //  we don't want to call 2 times the api since the first one can already obtain and cache
            //  the full secret with a TTL.
            match kv2::read::<SecretData>(&self.client, mount, path).await {
                Ok(secret) => {
                    if let Some(data) = secret.data.get(name) {
                        return Some(data.clone());
                    }
                }
                Err(e) => {
                    eprintln!("Error retrieving secret: {:?}", e);
                }
            }
            None
        })
    }
}
