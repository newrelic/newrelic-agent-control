pub mod env;
pub mod k8s_secret;
pub mod vault;

use crate::agent_type::variable::namespace::Namespace;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::secrets_provider::env::{Env, EnvError};
use crate::secrets_provider::k8s_secret::{K8sSecretProvider, K8sSecretProviderError};
use crate::secrets_provider::vault::{Vault, VaultConfig, VaultError};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

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
    #[error("vault provider failed: {0}")]
    VaultError(#[from] VaultError),

    #[error("k8s secret provider failed: {0}")]
    K8sSecretProviderError(#[from] K8sSecretProviderError),

    #[error("env var provider failed: {0}")]
    EnvError(#[from] EnvError),
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
    Env(Env),
}

impl SecretsProvider for SecretsProviderType {
    type Error = SecretsProvidersError;

    fn get_secret(&self, secret_path: &str) -> Result<String, Self::Error> {
        match self {
            SecretsProviderType::Vault(provider) => Ok(provider.get_secret(secret_path)?),
            SecretsProviderType::K8sSecret(provider) => Ok(provider.get_secret(secret_path)?),
            SecretsProviderType::Env(provider) => Ok(provider.get_secret(secret_path)?),
        }
    }
}

/// Collection of [SecretsProviderType]s.
#[derive(Default)]
pub struct SecretsProvidersRegistry(HashMap<Namespace, SecretsProviderType>);

impl SecretsProvidersRegistry {
    pub fn new() -> Self {
        SecretsProvidersRegistry(HashMap::new())
    }

    pub fn with_env(mut self) -> Self {
        self.0.insert(
            Namespace::EnvironmentVariable,
            SecretsProviderType::Env(Env {}),
        );
        self
    }

    pub fn with_k8s_secret(mut self, k8s_client: Arc<SyncK8sClient>) -> Self {
        self.0.insert(
            Namespace::K8sSecret,
            SecretsProviderType::K8sSecret(K8sSecretProvider::new(k8s_client)),
        );
        self
    }

    pub fn with_config(
        mut self,
        config: SecretsProvidersConfig,
    ) -> Result<Self, SecretsProvidersError> {
        if let Some(vault_config) = config.vault {
            let vault = Vault::try_build(vault_config)?;
            self.0
                .insert(Namespace::Vault, SecretsProviderType::Vault(vault));
        }
        Ok(self)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'a> IntoIterator for &'a SecretsProvidersRegistry {
    type Item = (&'a Namespace, &'a SecretsProviderType);
    type IntoIter = std::collections::hash_map::Iter<'a, Namespace, SecretsProviderType>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
