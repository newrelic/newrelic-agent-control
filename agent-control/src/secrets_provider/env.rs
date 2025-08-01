use crate::secrets_provider::SecretsProvider;
use anyhow::{Result, anyhow};

pub struct Env {}

impl SecretsProvider for Env {
    fn get_secret(&self, secret_path: &str) -> Result<String> {
        std::env::var(secret_path).map_err(|e| {
            anyhow!(format!(
                "failed to retrieve env var secret '{secret_path}': {e}"
            ))
        })
    }
}
