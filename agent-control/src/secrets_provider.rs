//! Secrets provider module
//!
//! This module defines the configuration and traits for secrets providers used in the agent control system.
//! It allows for flexible integration of various secrets providers, enabling retrieval of secrets from different sources.
//!
//! Adding support for a new secrets provider involves:
//!
//! * Adding a field to the [SecretsProvidersConfig] struct for the new provider's configuration.
//! * Implementing the [SecretsProviderBuilder] trait for the new provider's configuration.
//! * Updating the TryFrom implementation for [SecretsProvidersRegistry] to include the new provider.

pub mod vault;

use crate::secrets_provider::vault::{Vault, VaultConfig, VaultError, VaultSecretPath};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Debug;
use std::thread::sleep;
use std::time::Duration;
use http::header::InvalidHeaderValue;
use tracing::debug;
use crate::http::config::ProxyConfig;

const NR_VAULT:&str = "nr-vault";

/// Configuration for supported secrets providers.
///
/// Group of secrets providers configurations, that can be used to retrieve secrets from various sources.
/// All providers should be optional. This allow users to configure only the ones they need.
/// Besides, there no lower or upper limit on the number of providers that can be configured.
/// Users can retrieve secrets from secret provider "A" and secret provider "B" at the same time.
///
/// The structure is flexible enough to support multiple sources from the same provider.
/// This is a decision the implementer of the provider must make. This entails creating a "config"
/// represented as a [HashMap]. Augmenting the structure is simple. Example:
///
/// ```
/// # use std::collections::HashMap;
/// struct SecretsProvidersConfig {
///     new_provider: Option<NewProviderConfig>,
/// }
///
/// struct NewProviderConfig {
///    sources: HashMap<String, NewProviderSourceConfig>,
/// }
///
/// struct NewProviderSourceConfig {
///    // fields specific to the source
/// }
/// ```
///
/// Integrating support for new providers involves implementing the [SecretsProviderBuilder] trait for the new provider's
/// configuration.
///
/// In the future, secrets could be retrieved from additional sources such as cloud providers.
/// For example, AWS Secrets Manager, Azure Key Vault, etc.
#[derive(Debug, Default, Deserialize, PartialEq, Clone)]
pub struct SecretsProvidersConfig {
    pub vault: Option<VaultConfig>,
    pub proxy_config: ProxyConfig,
}

/// Trait for building a client to retrieve secrets from a provider.
///
/// Defines a method to build a client that implements the [SecretsProvider] trait.
/// The client can then be used for retrieving secrets from the provider or any other
/// supported operation.
pub trait SecretsProviderBuilder {
    type Provider: SecretsProvider;

    fn build_provider(&self) -> Result<Self::Provider, String>;
}

#[derive(Debug, thiserror::Error)]
pub enum SecretsProvidersError {
    #[error("Failed to retrieve secret from provider: {0}")]
    GetSecret(String),

    #[error("Invalid configuration for secrets provider: {0}")]
    InvalidProvider(String),

    #[error("Failed building Vault client: {0}")]
    VaultError(#[from] VaultError),
}

#[derive(Clone)]
pub enum SecretPath {
    Vault(VaultSecretPath),
    None,
}

/// Trait for operating with secrets providers.
///
/// Defines common operations among the different secrets providers.
pub trait SecretsProvider {
    type Error: Debug;

    fn get_secret(&self, secret_path: SecretPath) -> Result<String, Self::Error>;

    fn get_secret_with_retry(
        &self,
        limit: u64,
        retry_interval: Duration,
        secret_path: SecretPath,
    ) -> Result<String, Self::Error> {
        let mut secret = Ok("".to_string());
        for attempt in 1..=limit {
            debug!("Checking for secret with retries {attempt}/{limit}");
            secret = self.get_secret(secret_path.clone());
            match secret.as_ref() {
                Ok(secret) => {
                    return Ok(secret.clone());
                }
                Err(err) => {
                    debug!("Failure getting secret: {:?}", err);
                }
            }
            sleep(retry_interval);
        }
        secret
    }
}

/// Supported secrets providers.
///
/// Each variant must contain an implementation of the [SecretsProvider] trait.
///
/// The structure is flexible enough to support multiple sources from the same provider.
/// This is a decision the implementer of the provider must make. This entails creating a variant
/// represented as a [HashMap]. Augmenting the enum is simple. Example:
///
/// ```
/// # use std::collections::HashMap;
/// pub enum SecretsProviderKind {
///    NewKind(HashMap<String, NewProvider>),
/// }
/// # struct NewProvider {}
/// ```
///
/// The idea is that the [SecretsProvidersRegistry] holds a collection where each key is the name of the provider.
/// The value can either be an instance of the [SecretsProvider] trait or a collection. In the latter, the key is
/// the name of the source, and the value is an instance of [SecretsProvider].
pub enum SecretsProviderKind {
    Vault(Vault),
}

/// Collection of [SecretsProviderKind]s.
pub type SecretsProvidersRegistry = HashMap<String, SecretsProviderKind>;

impl TryFrom<SecretsProvidersConfig> for SecretsProvidersRegistry {
    type Error = SecretsProvidersError;

    /// Tries to convert a [SecretsProvidersConfig] into a [SecretsProvidersRegistry].
    ///
    /// If any of the configurations is invalid, it returns an error.
    fn try_from(config: SecretsProvidersConfig) -> Result<Self, Self::Error> {
        let mut registry = SecretsProvidersRegistry::new();

        if let Some(vault_config) = config.vault {
            let vault = Vault::try_build(vault_config, config.proxy_config)?;
            registry.insert(NR_VAULT.to_string(), SecretsProviderKind::Vault(vault));
        }

        Ok(registry)
    }
}

#[cfg(test)]
pub mod tests {
    use std::time::Duration;

    use super::*;
    use assert_matches::assert_matches;
    use mockall::{Sequence, mock};
    use thiserror::Error;

    #[derive(Error, Debug, PartialEq, Clone)]
    pub enum SecretProviderError {
        #[error("{0}")]
        Generic(String),
    }

    mock! {
        pub SecretProvider{}
        impl SecretsProvider for SecretProvider{
            type Error = SecretProviderError;

            fn get_secret(&self, secret_path: SecretPath) -> Result<String, SecretProviderError>;
        }
    }

    impl MockSecretProvider {
        pub fn new_secret() -> MockSecretProvider {
            let mut secret = MockSecretProvider::new();
            secret
                .expect_get_secret()
                .returning(|_path| Ok("a-secret".to_string()));
            secret
        }
    }

    #[test]
    fn test_get_secret_with_retry_success_on_first_attempt() {
        let secret_provider = MockSecretProvider::new_secret();

        let result =
            secret_provider.get_secret_with_retry(3, Duration::from_millis(10), SecretPath::None);

        assert_matches!(result, Ok(secret) => {
            assert_eq!("a-secret".to_string(), secret);
        });
    }

    #[test]
    fn test_get_secret_with_retry_success_after_retries() {
        let mut secret_provider = MockSecretProvider::new();
        let mut seq = Sequence::new();

        secret_provider
            .expect_get_secret()
            .once()
            .in_sequence(&mut seq)
            .returning(|_path| {
                Err(SecretProviderError::Generic(
                    "error on first attempt".to_string(),
                ))
            });
        secret_provider
            .expect_get_secret()
            .once()
            .in_sequence(&mut seq)
            .returning(|_path| {
                Err(SecretProviderError::Generic(
                    "error on second attempt".to_string(),
                ))
            });
        secret_provider
            .expect_get_secret()
            .once()
            .in_sequence(&mut seq)
            .returning(|_path| Ok("a-secret".to_string()));

        let result =
            secret_provider.get_secret_with_retry(3, Duration::from_millis(10), SecretPath::None);

        assert_matches!(result, Ok(secret) => {
            assert_eq!("a-secret".to_string(), secret);
        });
    }

    #[test]
    fn test_get_secret_with_retry_failure_after_all_attempts() {
        let mut secret_provider = MockSecretProvider::new();

        secret_provider
            .expect_get_secret()
            .times(3)
            .returning(|_path| Err(SecretProviderError::Generic("persistent error".to_string())));

        let result =
            secret_provider.get_secret_with_retry(3, Duration::from_millis(10), SecretPath::None);

        assert_matches!(result, Err(SecretProviderError::Generic(s)) => {
            assert_eq!(s, "persistent error".to_string());
        });
    }
}
