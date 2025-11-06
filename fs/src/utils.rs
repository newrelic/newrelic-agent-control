use std::path::{Component, Path};
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum FsError {
    #[error("invalid path: {0}")]
    InvalidPath(String),

    #[error("dots disallowed in path {0}")]
    DotsDisallowed(String),
}

pub fn validate_path(path: &Path) -> Result<(), FsError> {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        Err(FsError::DotsDisallowed(path.to_string_lossy().to_string()))
    } else if path.to_str().is_none() {
        Err(FsError::InvalidPath(format!(
            "{} is not valid unicode",
            path.display()
        )))
    } else {
        Ok(())
    }
}
