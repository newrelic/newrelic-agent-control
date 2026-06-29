//! Secrets provider that reads secrets from environment variables.

use crate::secrets_provider::SecretsProvider;

/// A secrets provider that retrieves secrets from environment variables.
pub struct Env {}

/// Error returned when an environment variable secret cannot be retrieved.
#[derive(Debug, thiserror::Error)]
#[error("failed to retrieve secret from environment variable: {0}")]
pub struct EnvError(String);

impl SecretsProvider for Env {
    type Error = EnvError;

    fn get_secret(&self, secret_path: &str) -> Result<String, Self::Error> {
        std::env::var(secret_path).map_err(|e| EnvError(e.to_string()))
    }
}
