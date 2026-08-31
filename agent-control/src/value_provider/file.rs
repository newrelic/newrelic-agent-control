//! Value provider that reads values from files on the local filesystem.

use std::fs;
use std::path::PathBuf;
use thiserror::Error;

use crate::value_provider::ValueProvider;

/// Error returned when a file value cannot be resolved.
#[derive(Debug, Error)]
#[error("resolving file value: {0}")]
pub struct FileProviderError(String);

/// A value provider that retrieves values from the local filesystem.
pub struct FileProvider;

impl ValueProvider for FileProvider {
    type Error = std::io::Error;

    fn get_value(&self, path: &str) -> Result<String, Self::Error> {
        let FilePath { path: file_path } = FilePath::try_from(path)?;
        fs::read_to_string(&file_path).map(|content| content.trim().to_string())
    }
}

/// Represents a File value path.
#[derive(Debug)]
pub struct FilePath {
    path: PathBuf,
}

/// Converts a raw string path into a [FilePath].
impl TryFrom<&str> for FilePath {
    type Error = std::io::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            return Err(std::io::Error::other("value path cannot be empty"));
        }

        Ok(FilePath {
            path: PathBuf::from(value),
        })
    }
}
