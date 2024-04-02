use regex::Regex;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum FsError {
    #[error("invalid path")]
    InvalidPath(),

    #[error("dots disallowed in path `{0}`")]
    DotsDisallowed(String),
}

#[cfg(target_family = "unix")]
pub fn validate_path(path: &Path) -> Result<(), FsError> {
    match path.to_str() {
        None => Err(FsError::InvalidPath()),
        Some(valid_path) => {
            // disallow dots
            let dots_regex = Regex::new(r"\.\.").unwrap();
            if dots_regex.is_match(valid_path) {
                Err(FsError::DotsDisallowed(valid_path.to_string()))
            } else {
                Ok(())
            }
        }
    }
}
