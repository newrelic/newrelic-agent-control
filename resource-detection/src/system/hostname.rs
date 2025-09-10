use super::detector::SystemDetectorError;
#[cfg(unix)]
use nix::unistd::gethostname;
use std::ffi::OsString;

/// wrapper for an hostname getter
#[derive(Default)]
pub struct HostnameGetter {}

impl HostnameGetter {
    #[cfg(target_family = "unix")]
    /// hostname getter
    pub fn get(&self) -> Result<OsString, SystemDetectorError> {
        gethostname().map_err(|e| SystemDetectorError::HostnameError(e.to_string()))
    }

    #[cfg(target_family = "windows")]
    /// hostname getter
    pub fn get(&self) -> Result<OsString, SystemDetectorError> {
        unimplemented!("")
    }
}
