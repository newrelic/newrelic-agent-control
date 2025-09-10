use super::detector::SystemDetectorError;

#[cfg(target_family = "unix")]
/// hostname getter
pub fn get_hostname() -> Result<String, SystemDetectorError> {
    use nix::unistd::gethostname;
    gethostname()
        .map_err(|e| SystemDetectorError::HostnameError(e.to_string()))
        .map(|h| h.into_string().unwrap_or_default())
}

#[cfg(target_family = "windows")]
/// hostname getter
pub fn get_hostname() -> Result<String, SystemDetectorError> {
    unimplemented!("")
}
