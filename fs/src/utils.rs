use regex::Regex;
use std::fs::Permissions;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum FsError {
    #[error("invalid path: `{0}`")]
    InvalidPath(String),

    #[error("dots disallowed in path `{0}`")]
    DotsDisallowed(String),
}

#[cfg(target_family = "unix")]
pub fn validate_path(path: &Path) -> Result<(), FsError> {
    match path.to_str() {
        None => Err(FsError::InvalidPath(format!(
            "{} is not valid unicode",
            path.to_string_lossy()
        ))),
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

#[cfg(target_family = "windows")]
pub fn validate_path(_path: &Path) -> Result<(), FsError> {
    unimplemented!()
}

#[cfg(target_family = "unix")]
pub fn get_file_permissions() -> Permissions {
    Permissions::from_mode(0o600)
}

#[cfg(target_family = "unix")]
pub fn get_directory_permissions() -> Permissions {
    Permissions::from_mode(0o700)
}

#[cfg(target_family = "windows")]
pub fn get_file_permissions() -> Permissions {
    unimplemented!()
}

#[cfg(target_family = "windows")]
pub fn get_directory_permissions() -> Permissions {
    unimplemented!()
}

#[cfg(target_family = "unix")]
pub fn get_pid_file_permissions() -> Permissions {
    Permissions::from_mode(0o644)
}

#[cfg(target_family = "unix")]
pub fn get_pid_directory_permissions() -> Permissions {
    Permissions::from_mode(0o755)
}

#[cfg(target_family = "windows")]
pub fn get_pid_file_permissions() -> Permissions {
    unimplemented!()
}

#[cfg(target_family = "windows")]
pub fn get_pid_directory_permissions() -> Permissions {
    unimplemented!()
}
