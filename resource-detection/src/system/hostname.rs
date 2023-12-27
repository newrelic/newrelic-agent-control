use super::detector::SystemDetectorError;
#[cfg(unix)]
use nix::unistd::gethostname;
use std::ffi::OsString;

#[derive(Default)]
pub struct HostnameGetter {}

#[cfg_attr(test, mockall::automock)]
impl HostnameGetter {
    #[cfg(unix)]
    pub fn get(&self) -> Result<OsString, SystemDetectorError> {
        gethostname().map_err(|e| SystemDetectorError::HostnameError(e.to_string()))
    }
}
