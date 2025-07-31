use crate::secrets_provider::SecretsProvider;

pub struct Env {}

#[derive(Debug, thiserror::Error)]
#[error("failed to retrieve secret from environment variable: {0}")]
pub struct EnvError(String);

impl SecretsProvider for Env {
    type Error = EnvError;

    fn get_secret(&self, secret_path: &str) -> Result<String, Self::Error> {
        std::env::var(secret_path).map_err(|e| EnvError(e.to_string()))
    }
}
