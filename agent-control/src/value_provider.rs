//! Value providers: retrieve variable values from various sources (env vars, files, Kubernetes, Vault, Azure Key Vault, AWS Secrets Manager, Google Cloud Secret Manager).

pub mod aws_secrets_manager;
pub mod azure_key_vault;
pub mod env;
pub mod file;
pub mod gcp_secret_manager;
pub mod k8s_configmap;
pub mod k8s_secret;
pub mod vault;

use crate::agent_type::variable::namespace::Namespace;
use crate::http::config::ProxyConfig;
use crate::k8s::client::{K8sClient, SyncK8sClient};
use crate::value_provider::aws_secrets_manager::{AwsSecretsManager, AwsSecretsManagerConfig};
use crate::value_provider::azure_key_vault::{AzureKeyVault, AzureKeyVaultConfig};
use crate::value_provider::env::Env;
use crate::value_provider::file::FileProvider;
use crate::value_provider::gcp_secret_manager::{GcpSecretManager, GcpSecretManagerConfig};
use crate::value_provider::k8s_configmap::K8sConfigMapProvider;
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
    /// Optional configuration for the Azure Key Vault provider.
    pub azure_key_vault: Option<AzureKeyVaultConfig>,
    /// Optional configuration for the AWS Secrets Manager provider.
    pub aws_secrets_manager: Option<AwsSecretsManagerConfig>,
    /// Optional configuration for the Google Cloud Secret Manager provider.
    pub gcp_secret_manager: Option<GcpSecretManagerConfig>,
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
    /// Values retrieved from AWS Secrets Manager.
    AwsSecretsManager(AwsSecretsManager),
    /// Values retrieved from Azure Key Vault.
    AzureKeyVault(AzureKeyVault),
    /// Values retrieved from Google Cloud Secret Manager.
    GcpSecretManager(GcpSecretManager),
    /// Values retrieved from HashiCorp Vault.
    Vault(Vault),
    /// Values retrieved from Kubernetes secrets.
    K8sSecret(K8sSecretProvider<C>),
    /// Values retrieved from Kubernetes ConfigMaps.
    K8sConfigMap(K8sConfigMapProvider<C>),
    /// Values retrieved from the local filesystem.
    File(FileProvider),
    /// Values retrieved from environment variables.
    Env(Env),
}

impl<C: K8sClient> ValueProvider for ValueProviderType<C> {
    type Error = ValueProvidersError;

    fn get_value(&self, path: &str) -> Result<String, Self::Error> {
        match self {
            ValueProviderType::AwsSecretsManager(provider) => {
                provider.get_value(path).map_err(|err| {
                    ValueProvidersError(format!("aws secrets manager provider failed: {err}"))
                })
            }
            ValueProviderType::AzureKeyVault(provider) => provider.get_value(path).map_err(|err| {
                ValueProvidersError(format!("azure key vault provider failed: {err}"))
            }),
            ValueProviderType::GcpSecretManager(provider) => {
                provider.get_value(path).map_err(|err| {
                    ValueProvidersError(format!("gcp secret manager provider failed: {err}"))
                })
            }
            ValueProviderType::Vault(provider) => provider
                .get_value(path)
                .map_err(|err| ValueProvidersError(format!("vault provider failed: {err}"))),
            ValueProviderType::K8sSecret(provider) => provider
                .get_value(path)
                .map_err(|err| ValueProvidersError(format!("k8s secret provider failed: {err}"))),
            ValueProviderType::K8sConfigMap(provider) => provider.get_value(path).map_err(|err| {
                ValueProvidersError(format!("k8s configmap provider failed: {err}"))
            }),
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

    /// Registers the Kubernetes ConfigMap provider backed by the given client.
    pub fn with_k8s_configmap(mut self, k8s_client: Arc<SyncK8sClient>) -> Self {
        self.0.insert(
            Namespace::K8sConfigMap,
            ValueProviderType::K8sConfigMap(K8sConfigMapProvider::new(k8s_client)),
        );
        self
    }

    /// Registers the file provider.
    pub fn with_file(mut self) -> Self {
        self.0
            .insert(Namespace::File, ValueProviderType::File(FileProvider));
        self
    }

    /// Registers providers derived from the given configuration (Vault, Azure Key Vault, AWS Secrets Manager, Google Cloud Secret Manager).
    pub fn with_config(
        mut self,
        config: ValueProvidersConfig,
        proxy_config: ProxyConfig,
    ) -> Result<Self, ValueProvidersError> {
        if let Some(mut vault_config) = config.vault {
            vault_config.proxy_config = proxy_config.clone();
            let vault = Vault::try_build(vault_config).map_err(|err| {
                ValueProvidersError(format!("couldn't build vault provider: {err}"))
            })?;
            self.0
                .insert(Namespace::Vault, ValueProviderType::Vault(vault));
        }
        if let Some(mut akv_config) = config.azure_key_vault {
            akv_config.proxy_config = proxy_config.clone();
            let akv = AzureKeyVault::try_build(akv_config).map_err(|err| {
                ValueProvidersError(format!("couldn't build azure key vault provider: {err}"))
            })?;
            self.0.insert(
                Namespace::AzureKeyVault,
                ValueProviderType::AzureKeyVault(akv),
            );
        }
        if let Some(mut gsm_config) = config.gcp_secret_manager {
            gsm_config.proxy_config = proxy_config.clone();
            let gsm = GcpSecretManager::try_build(gsm_config).map_err(|err| {
                ValueProvidersError(format!("couldn't build gcp secret manager provider: {err}"))
            })?;
            self.0.insert(
                Namespace::GcpSecretManager,
                ValueProviderType::GcpSecretManager(gsm),
            );
        }
        if let Some(mut asm_config) = config.aws_secrets_manager {
            asm_config.proxy_config = proxy_config;
            let asm = AwsSecretsManager::try_build(asm_config).map_err(|err| {
                ValueProvidersError(format!(
                    "couldn't build aws secrets manager provider: {err}"
                ))
            })?;
            self.0.insert(
                Namespace::AwsSecretsManager,
                ValueProviderType::AwsSecretsManager(asm),
            );
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
