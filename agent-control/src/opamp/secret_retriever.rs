//! Retrieval of the OpAMP authentication secret (the private key) across environments.

pub mod k8s;
pub mod on_host;
/// Abstraction for retrieving the OpAMP authentication secret (Private Key).
///
/// This trait allows unifying the secret retrieval logic regardless of the
/// execution environment (Kubernetes or On-Host).
pub trait OpampSecretRetriever {
    /// Error type returned when retrieval fails.
    type Error: std::error::Error;
    /// Retrieves the content of the secret (the private key).
    ///
    /// The specific retrieval strategy (e.g., reading a local file or querying the
    /// Kubernetes API) and the location of the secret are determined by the
    /// implementation's internal state, configured during its initialization.
    fn retrieve(&self) -> Result<String, Self::Error>;
}
#[cfg(test)]
#[allow(missing_docs)]
pub mod test_mocks {
    use crate::value_provider::ValueProvider;
    use mockall::mock;

    mock! {
        pub ValueProvider {}
        impl ValueProvider for ValueProvider {
            type Error = std::io::Error;
            fn get_value(&self, path: &str) -> Result<String, std::io::Error>;
        }
    }
}
