pub mod env;
pub mod k8s_secret;
pub mod vault;

use crate::agent_type::variable::namespace::Namespace;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::secrets_provider::env::Env;
use crate::secrets_provider::k8s_secret::K8sSecretProvider;
use crate::secrets_provider::vault::{Vault, VaultConfig};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use anyhow::Result;

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

/// Trait for operating with secrets providers.
///
/// Defines common operations among the different secrets providers.
pub trait SecretsProvider {
    /// Gets a secret
    /// By default is recommended to use get_secret_with_retry.
    fn get_secret(&self, secret_path: &str) -> Result<String>;
}

#[derive(Default)]
pub struct SecretsProviders(HashMap<Namespace, Box<dyn SecretsProvider + Send + Sync>>);

impl SecretsProviders {
    pub fn new() -> Self {
        SecretsProviders(HashMap::new())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn with_env(mut self) -> Self {
        self.0
            .insert(Namespace::EnvironmentVariable, Box::new(Env {}));
        self
    }

    pub fn with_k8s_secret(mut self, k8s_client: Arc<SyncK8sClient>) -> Self {
        self.0.insert(
            Namespace::K8sSecret,
            Box::new(K8sSecretProvider::new(k8s_client)),
        );
        self
    }

    pub fn with_config(mut self, config: SecretsProvidersConfig) -> Result<Self> {
        if let Some(vault_config) = config.vault {
            let vault = Vault::try_build(vault_config)?;
            self.0.insert(Namespace::Vault, Box::new(vault));
        }
        Ok(self)
    }
}

impl<'a> IntoIterator for &'a SecretsProviders {
    type Item = (&'a Namespace, &'a Box<dyn SecretsProvider + Send + Sync>);
    type IntoIter =
        std::collections::hash_map::Iter<'a, Namespace, Box<dyn SecretsProvider + Send + Sync>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cfg(test)]
impl From<HashMap<Namespace, Box<dyn SecretsProvider + Send + Sync>>> for SecretsProviders {
    fn from(value: HashMap<Namespace, Box<dyn SecretsProvider + Send + Sync>>) -> Self {
        Self(value)
    }
}
