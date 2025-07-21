pub mod vault;

use crate::secrets_provider::vault::{Vault, VaultConfig, VaultError, VaultSecretPath};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Debug;
use std::thread::sleep;
use std::time::Duration;
use tracing::debug;

const NR_VAULT: &str = "nr-vault";

/// Configuration for supported secrets providers.
///
/// Group of secrets providers configurations, that can be used to retrieve secrets from various sources.
/// All providers should be optional. This allows users to configure only the ones they need.
/// Besides, there is no lower or upper limit on the number of providers that can be configured.
/// Users can retrieve secrets from secret provider "A" and secret provider "B" at the same time.
///
/// The structure is flexible enough to support multiple sources from the same provider.
/// This is a decision the implementer of the provider must make. This entails creating a "config"
/// represented as a [HashMap]. Augmenting the structure is simple.
///
/// Example:
///
/// ```
/// # use std::collections::HashMap;
/// struct SecretsProvidersConfig {
///     new_provider: Option<NewProviderConfig>,
/// }
///
/// struct NewProviderConfig {}
/// ```
///
/// Integrating support for new providers involves implementing the [SecretsProviderBuilder] trait for the new provider's
/// configuration.
#[derive(Debug, Default, Deserialize, PartialEq, Clone)]
pub struct SecretsProvidersConfig {
    pub vault: Option<VaultConfig>,
}

/// Trait for building a client to retrieve secrets from a provider.
///
/// Defines a method to build a client that implements the [SecretsProvider] trait.
/// The client can then be used for retrieving secrets from the provider or any other
/// supported operation.
pub trait SecretsProviderBuilder {
    type Provider: SecretsProvider;
    type Error: Debug;

    fn build_provider(&self) -> Result<Self::Provider, Self::Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum SecretsProvidersError {
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
    type Error: Debug + From<String>;

    /// Gets a secret
    /// By default is recommended to use get_secret_with_retry.
    fn get_secret(&self, secret_path: SecretPath) -> Result<String, Self::Error>;

    /// Gets a secret with a retry policy
    fn get_secret_with_retry(
        &self,
        limit: u64,
        retry_interval: Duration,
        secret_path: SecretPath,
    ) -> Result<String, Self::Error> {
        for attempt in 1..=limit {
            debug!("Checking for secret with retries {attempt}/{limit}");
            match self.get_secret(secret_path.clone()) {
                Ok(secret) => {
                    return Ok(secret.clone());
                }
                Err(err) => {
                    debug!("Failure getting secret: {:?}", err);
                }
            }
            sleep(retry_interval);
        }
        Err("Failed to retrieve secret after all retry attempts"
            .to_string()
            .into())
    }
}

/// Supported secrets providers.
///
/// Each variant must contain an implementation of the [SecretsProvider] trait.
///
/// The structure is flexible enough to support multiple sources from the same provider.
/// This is a decision the implementer of the provider must make. This entails creating a variant
/// represented as a [HashMap]. Augmenting the enum is simple. Example:
pub enum SecretsProviderType {
    Vault(Vault),
}

/// Collection of [SecretsProviderType]s.
pub type SecretsProvidersRegistry = HashMap<String, SecretsProviderType>;

impl TryFrom<SecretsProvidersConfig> for SecretsProvidersRegistry {
    type Error = SecretsProvidersError;

    /// Tries to convert a [SecretsProvidersConfig] into a [SecretsProvidersRegistry].
    ///
    /// If any of the configurations is invalid, it returns an error.
    fn try_from(config: SecretsProvidersConfig) -> Result<Self, Self::Error> {
        let mut registry = SecretsProvidersRegistry::new();

        if let Some(vault_config) = config.vault {
            let vault = vault_config.build_provider()?;
            registry.insert(NR_VAULT.to_string(), SecretsProviderType::Vault(vault));
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
        GenericError(String),
    }

    impl From<String> for SecretProviderError {
        fn from(s: String) -> Self {
            SecretProviderError::GenericError(s)
        }
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
                Err(SecretProviderError::GenericError(
                    "error on first attempt".to_string(),
                ))
            });
        secret_provider
            .expect_get_secret()
            .once()
            .in_sequence(&mut seq)
            .returning(|_path| {
                Err(SecretProviderError::GenericError(
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
            .returning(|_path| Err(SecretProviderError::GenericError("an error".to_string())));

        let result =
            secret_provider.get_secret_with_retry(3, Duration::from_millis(10), SecretPath::None);

        assert_matches!(result, Err(SecretProviderError::GenericError(s)) => {
            assert_eq!(s, "Failed to retrieve secret after all retry attempts".to_string());
        });
    }
}
