use crate::secrets_provider::{SecretsProvider, SecretsProvidersError};

pub struct Env {}

#[derive(Debug, thiserror::Error)]
#[error("failed to retrieve secret from environment variable: {0}")]
pub struct EnvError(String);

impl SecretsProvider for Env {
    fn get_secret(&self, secret_path: &str) -> Result<String, SecretsProvidersError> {
        std::env::var(secret_path)
            .map_err(|e| SecretsProvidersError::EnvError(EnvError(e.to_string())))
    }
}
