use crate::secrets_provider::SecretsProvider;
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("reading file secret: {0}")]
pub struct FileSecretProviderError(String);

/// A secrets provider that retrieves secrets from the local filesystem.
pub struct FileSecretProvider {
    /// The base directory where secrets are located.
    base_path: PathBuf,
}

impl FileSecretProvider {
    /// Creates a new provider rooted at the specified directory.
    pub fn new(base_path: PathBuf) -> Self {
        FileSecretProvider { base_path }
    }
}

impl SecretsProvider for FileSecretProvider {
    type Error = FileSecretProviderError;

    /// Retrieves the content of a file.
    ///
    /// `secret_path` is treated as the filename relative to the provider's base_path.
    /// Example: if base_path is "/etc/secrets" and secret_path is "private.key",
    /// it reads "/etc/secrets/private.key".
    fn get_secret(&self, secret_name: &str) -> Result<String, Self::Error> {
        let full_path = self.base_path.join(secret_name);

        fs::read_to_string(&full_path).map_err(|err| {
            FileSecretProviderError(format!(
                "failed to read secret file '{}': {}",
                full_path.display(),
                err
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;
    #[test]
    fn test_get_secret_success() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("my-secret.key");

        let secret_content = "super-secret-value";
        let mut file = File::create(&file_path).unwrap();
        write!(file, "{}", secret_content).unwrap();

        let provider = FileSecretProvider::new(dir.path().to_path_buf());

        let result = provider.get_secret("my-secret.key");

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), secret_content);
    }

    #[test]
    fn test_get_secret_not_found() {
        let dir = tempdir().unwrap();
        let provider = FileSecretProvider::new(dir.path().to_path_buf());

        let result = provider.get_secret("non-existent-file");

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("failed to read secret file")
        );
    }
}
