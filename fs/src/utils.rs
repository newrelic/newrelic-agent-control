use std::{
    io,
    path::{Component, Path},
};

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
