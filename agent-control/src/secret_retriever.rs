pub mod k8s;
pub mod on_host;
/// Abstraction for retrieving the OpAMP authentication secret (Private Key).
///
/// This trait allows unifying the secret retrieval logic regardless of the
/// execution environment (Kubernetes or On-Host).
pub trait OpampSecretRetriever {
    type Error: std::error::Error;
    /// Retrieves the content of the secret (the private key).
    ///
    /// The specific retrieval strategy (e.g., reading a local file or querying the
    /// Kubernetes API) and the location of the secret are determined by the
    /// implementation's internal state, configured during its initialization.
    fn retrieve(&self) -> Result<String, Self::Error>;
}
#[cfg(test)]
pub mod test_mocks {
    use crate::secrets_provider::SecretsProvider;
    use mockall::mock;

    mock! {
        pub SecretsProvider {}
        impl SecretsProvider for SecretsProvider {
            type Error = std::io::Error;
            fn get_secret(&self, secret_path: &str) -> Result<String, std::io::Error>;
        }
    }
}
