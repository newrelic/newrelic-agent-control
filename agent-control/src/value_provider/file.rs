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
#[derive(Default)]
pub struct FileProvider;

impl FileProvider {
    /// Creates a new [`FileProvider`].
    pub fn new() -> Self {
        FileProvider
    }

    /// Helper to construct the path string expected by get_value.
    /// In this case, it just returns the path as a string.
    pub fn build_path(path: &str) -> String {
        path.to_string()
    }
}

impl ValueProvider for FileProvider {
    type Error = FileProviderError;

    fn get_value(&self, path: &str) -> Result<String, Self::Error> {
        let FilePath { path: file_path } = FilePath::try_from(path)?;

        fs::read_to_string(&file_path)
            .map(|content| content.trim().to_string())
            .map_err(|err| FileProviderError(format!("reading '{path}' value: {err}")))
    }
}

/// Represents a File value path.
#[derive(Debug)]
pub struct FilePath {
    path: PathBuf,
}

/// Converts a raw string path into a [FilePath].
impl TryFrom<&str> for FilePath {
    type Error = FileProviderError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            return Err(FileProviderError("value path cannot be empty".to_string()));
        }

        Ok(FilePath {
            path: PathBuf::from(value),
        })
    }
}
