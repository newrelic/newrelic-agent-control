use std::{
    io,
    path::{Component, Path},
};
use thiserror::Error;

/// Errors produced by the filesystem helpers in this crate.
#[derive(Error, Debug, Clone)]
pub enum FsError {
    /// The path is not valid (for example, it is not valid Unicode).
    #[error("invalid path: {0}")]
    InvalidPath(String),

    /// The path contains `..` (parent-directory) components, which are disallowed.
    #[error("dots disallowed in path {0}")]
    DotsDisallowed(String),
}

/// Rejects paths that contain `..` components or are not valid Unicode.
///
/// Returns an [`io::Error`] of kind [`io::ErrorKind::InvalidInput`] when the path is
/// disallowed; otherwise returns `Ok(())`.
pub fn validate_path(path: &Path) -> io::Result<()> {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("dots disallowed in path {}", path.to_string_lossy()),
        ))
    } else if path.to_str().is_none() {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not valid unicode", path.display()),
        ))
    } else {
        Ok(())
    }
}
