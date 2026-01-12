use std::fs;
use std::path::PathBuf;
use thiserror::Error;

use crate::secrets_provider::SecretsProvider;

#[derive(Debug, Error)]
#[error("resolving file secret: {0}")]
pub struct FileSecretProviderError(String);

/// A secrets provider that retrieves secrets from the local filesystem.
#[derive(Default)]
pub struct FileSecretProvider;

impl FileSecretProvider {
    pub fn new() -> Self {
        FileSecretProvider
    }

    /// Helper to construct the secret path string expected by get_secret.
    /// In this case, it just returns the path as a string.
    pub fn build_secret_path(path: &str) -> String {
        path.to_string()
    }
}

impl SecretsProvider for FileSecretProvider {
    type Error = FileSecretProviderError;

    fn get_secret(&self, secret_path: &str) -> Result<String, Self::Error> {
        let FileSecretPath { path } = FileSecretPath::try_from(secret_path)?;

        fs::read_to_string(&path)
            .map(|content| content.trim().to_string())
            .map_err(|err| {
                FileSecretProviderError(format!("reading '{secret_path}' secret: {err}"))
            })
    }
}

/// Represents a File secret path.
#[derive(Debug)]
pub struct FileSecretPath {
    path: PathBuf,
}

/// Converts a raw string path into a [FileSecretPath].
impl TryFrom<&str> for FileSecretPath {
    type Error = FileSecretProviderError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            return Err(FileSecretProviderError(
                "secret path cannot be empty".to_string(),
            ));
        }

        Ok(FileSecretPath {
            path: PathBuf::from(value),
        })
    }
}
