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
pub enum SecretsProviderType {}

/// Collection of [SecretsProviderType]s.
pub type SecretsProviders = HashMap<String, SecretsProviderType>;

#[derive(Debug, thiserror::Error)]
pub enum SecretsProvidersConfigError {
    #[error("Invalid configuration for secrets provider: {0}")]
    InvalidProvider(String),
}

impl TryFrom<SecretsProvidersConfig> for SecretsProviders {
    type Error = SecretsProvidersConfigError;

    /// Tries to convert a [SecretsProvidersConfig] into a [SecretsProviders].
    ///
    /// If any of the configurations is invalid, it returns an error.
    fn try_from(config: SecretsProvidersConfig) -> Result<Self, Self::Error> {
        Ok(HashMap::new())
    }
}

