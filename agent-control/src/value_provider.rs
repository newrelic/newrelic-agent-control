//! Value providers: retrieve variable values from various sources (env vars, files, Kubernetes, Vault).

pub mod env;
pub mod file;
pub mod k8s_secret;
pub mod vault;

use crate::agent_type::variable::namespace::Namespace;
use crate::k8s::client::{K8sClient, SyncK8sClient};
use crate::value_provider::env::Env;
use crate::value_provider::file::FileProvider;
use crate::value_provider::k8s_secret::K8sSecretProvider;
use crate::value_provider::vault::{Vault, VaultConfig};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

/// Configuration for supported value providers.
///
/// Group of value providers configurations, that can be used to retrieve values from various sources.
/// All providers should be optional. This allows users to configure only the ones they need.
/// Besides, there is no lower or upper limit on the number of providers that can be configured.
/// Users can retrieve values from value provider "A" and value provider "B" at the same time.
///
/// The structure is flexible enough to support multiple sources from the same provider.
/// This is a decision the implementer of the provider must make. This entails creating a "config"
/// represented as a [HashMap]. Augmenting the structure is simple.
///
/// Example:
///
/// ```
/// # use std::collections::HashMap;
/// struct ValueProvidersConfig {
///     new_provider: Option<NewProviderConfig>,
/// }
///
/// struct NewProviderConfig {}
/// ```
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
pub struct ValueProvidersConfig {
    /// Optional configuration for the HashiCorp Vault provider.
    pub vault: Option<VaultConfig>,
}

/// Errors returned by the configured value providers.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ValueProvidersError(String);

/// Trait for operating with value providers.
///
/// Defines common operations among the different value providers.
pub trait ValueProvider {
    /// Error type returned when retrieving a value fails.
    type Error: std::error::Error;

    /// Gets a value.
    fn get_value(&self, path: &str) -> Result<String, Self::Error>;
}

/// Supported value providers.
///
/// Each variant must contain an implementation of the [ValueProvider] trait.
///
/// The structure is flexible enough to support multiple sources from the same provider.
/// This is a decision the implementer of the provider must make. This entails creating a variant
/// represented as a [HashMap].
pub enum ValueProviderType<C: K8sClient = SyncK8sClient> {
    /// Values retrieved from HashiCorp Vault.
    Vault(Vault),
    /// Values retrieved from Kubernetes secrets.
    K8sSecret(K8sSecretProvider<C>),
    /// Values retrieved from the local filesystem.
    File(FileProvider),
    /// Values retrieved from environment variables.
    Env(Env),
}

impl<C: K8sClient> ValueProvider for ValueProviderType<C> {
    type Error = ValueProvidersError;

    fn get_value(&self, path: &str) -> Result<String, Self::Error> {
        match self {
            ValueProviderType::Vault(provider) => provider
                .get_value(path)
                .map_err(|err| ValueProvidersError(format!("vault provider failed: {err}"))),
            ValueProviderType::K8sSecret(provider) => provider
                .get_value(path)
                .map_err(|err| ValueProvidersError(format!("k8s secret provider failed: {err}"))),
            ValueProviderType::File(provider) => provider
                .get_value(path)
                .map_err(|err| ValueProvidersError(format!("file provider failed: {err}"))),
            ValueProviderType::Env(provider) => provider
                .get_value(path)
                .map_err(|err| ValueProvidersError(format!("env provider failed: {err}"))),
        }
    }
}

/// Collection of [ValueProviderType]s.
pub type ValueProviders<C = SyncK8sClient> = Registry<ValueProviderType<C>>;

/// A collection of value providers keyed by [`Namespace`].
pub struct Registry<S: ValueProvider>(HashMap<Namespace, S>);

impl<S: ValueProvider> Registry<S> {
    /// Returns `true` if no providers are registered.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Default for Registry<ValueProviderType> {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl Registry<ValueProviderType> {
    /// Registers the environment variable provider.
    pub fn with_env(mut self) -> Self {
        self.0.insert(
            Namespace::EnvironmentVariable,
            ValueProviderType::Env(Env {}),
        );
        self
    }

    /// Registers the Kubernetes secret provider backed by the given client.
    pub fn with_k8s_secret(mut self, k8s_client: Arc<SyncK8sClient>) -> Self {
        self.0.insert(
            Namespace::K8sSecret,
            ValueProviderType::K8sSecret(K8sSecretProvider::new(k8s_client)),
        );
        self
    }

    /// Registers the file provider.
    pub fn with_file(mut self) -> Self {
        self.0
            .insert(Namespace::File, ValueProviderType::File(FileProvider));
        self
    }

    /// Registers providers derived from the given configuration (currently Vault).
    pub fn with_config(
        mut self,
        config: ValueProvidersConfig,
    ) -> Result<Self, ValueProvidersError> {
        if let Some(vault_config) = config.vault {
            let vault = Vault::try_build(vault_config).map_err(|err| {
                ValueProvidersError(format!("couldn't build vault provider: {err}"))
            })?;
            self.0
                .insert(Namespace::Vault, ValueProviderType::Vault(vault));
        }
        Ok(self)
    }
}

impl<'a, S: ValueProvider> IntoIterator for &'a Registry<S> {
    type Item = (&'a Namespace, &'a S);
    type IntoIter = std::collections::hash_map::Iter<'a, Namespace, S>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cfg(test)]
impl<S: ValueProvider> From<HashMap<Namespace, S>> for Registry<S> {
    fn from(value: HashMap<Namespace, S>) -> Self {
        Self(value)
    }
}
