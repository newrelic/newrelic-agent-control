pub mod k8s_secret;
pub mod vault;

use crate::agent_type::variable::namespace::Namespace;
use crate::secrets_provider::k8s_secret::K8sSecretProvider;
use crate::secrets_provider::vault::{Vault, VaultConfig, VaultError};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Debug;
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
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
pub struct SecretsProvidersConfig {
    pub vault: Option<VaultConfig>,
}

#[derive(Debug, thiserror::Error)]
pub enum SecretsProvidersError {
    #[error("Failed building Vault client: {0}")]
    VaultError(#[from] VaultError),
}

/// Trait for operating with secrets providers.
///
/// Defines common operations among the different secrets providers.
pub trait SecretsProvider {
    type Error: std::error::Error;

    /// Gets a secret
    /// By default is recommended to use get_secret_with_retry.
    fn get_secret(&self, secret_path: &str) -> Result<String, Self::Error>;
}

/// Supported secrets providers.
///
/// Each variant must contain an implementation of the [SecretsProvider] trait.
///
/// The structure is flexible enough to support multiple sources from the same provider.
/// This is a decision the implementer of the provider must make. This entails creating a variant
/// represented as a [HashMap].
pub enum SecretsProviderType {
    Vault(Vault),
    K8sSecret(K8sSecretProvider),
}

/// Collection of [SecretsProviderType]s.
pub type SecretsProvidersRegistry = HashMap<Namespace, SecretsProviderType>;

impl TryFrom<SecretsProvidersConfig> for SecretsProvidersRegistry {
    type Error = SecretsProvidersError;

    /// Tries to convert a [SecretsProvidersConfig] into a [SecretsProvidersRegistry].
    ///
    /// If any of the configurations is invalid, it returns an error.
    fn try_from(config: SecretsProvidersConfig) -> Result<Self, Self::Error> {
        let mut registry = SecretsProvidersRegistry::new();

        if let Some(vault_config) = config.vault {
            let vault = Vault::try_build(vault_config)?;
            registry.insert(Namespace::Vault, SecretsProviderType::Vault(vault));
        }

        Ok(registry)
    }
}
