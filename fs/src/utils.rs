use std::{
    io,
    path::{Component, Path},
};
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum FsError {
    #[error("invalid path: {0}")]
    InvalidPath(String),

    #[error("dots disallowed in path {0}")]
    DotsDisallowed(String),
}

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
