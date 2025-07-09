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

use std::collections::HashMap;

use serde::Deserialize;

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
pub struct SecretsProvidersConfig {}

/// Trait for building a client to retrieve secrets from a provider.
///
/// Defines a method to build a client that implements the [SecretsProvider] trait.
/// The client can then be used for retrieving secrets from the provider or any other
/// supported operation.
pub trait SecretsProviderBuilder {
    type Provider: SecretsProvider;

    fn build_provider(&self) -> Result<Self::Provider, String>;
}

/// Trait for operating with secrets providers.
///
/// Defines common operations among the different secrets providers.
pub trait SecretsProvider {
    fn get_secret(&self, mount: &str, path: &str, key: &str) -> Result<String, String>;
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
///    NewProvider(HashMap<String, NewProvider>),
/// }
/// ```
/// 
/// The idea is that the [SecretsProvidersRegistry] holds a collection where each key is the name of the provider.
/// The value can either be an instance of the [SecretsProvider] trait or a collection. In the latter, the key is
/// the name of the source, and the value is an instance of [SecretsProvider].
pub enum SecretsProviderKind {}

/// Collection of [SecretsProviderKind]s.
pub type SecretsProvidersRegistry = HashMap<String, SecretsProviderKind>;

#[derive(Debug, thiserror::Error)]
pub enum SecretsProvidersError {
    #[error("Invalid configuration for secrets provider: {0}")]
    InvalidProvider(String),
}

impl TryFrom<SecretsProvidersConfig> for SecretsProvidersRegistry {
    type Error = SecretsProvidersError;

    /// Tries to convert a [SecretsProvidersConfig] into a [SecretsProvidersRegistry].
    ///
    /// If any of the configurations is invalid, it returns an error.
    fn try_from(config: SecretsProvidersConfig) -> Result<Self, Self::Error> {
        Ok(HashMap::new())
    }
}
