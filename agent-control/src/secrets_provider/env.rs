use std::error::Error;
use std::fmt;

use crate::secrets_provider::SecretsProvider;

#[derive(Debug, Clone)]
struct EnvError(String, String);

impl fmt::Display for EnvError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "failed to retrieve env var secret '{}': {}",
            self.0, self.1
        )
    }
}

impl Error for EnvError {}

pub struct Env {}

impl SecretsProvider for Env {
    fn get_secret(&self, secret_path: &str) -> Result<String, Box<dyn Error>> {
        std::env::var(secret_path)
            .map_err(|e| EnvError(secret_path.to_string(), e.to_string()).into())
    }
}
