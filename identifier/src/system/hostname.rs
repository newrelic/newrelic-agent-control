#[cfg(unix)]
use nix::errno::Errno;
#[cfg(unix)]
use nix::unistd::gethostname;
use std::ffi::OsString;

#[derive(Default)]
pub struct HostnameGetter {}

#[cfg_attr(test, mockall::automock)]
impl HostnameGetter {
    #[cfg(unix)]
    pub fn get(&self) -> Result<OsString, Errno> {
        gethostname()
    }
}
