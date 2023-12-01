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

#[cfg(test)]
mod test {
    use crate::utils::hostname::MockHostnameGetter;
    use nix::errno::Errno;
    use std::ffi::OsString;

    impl MockHostnameGetter {
        pub fn should_get(&mut self, hostname: String) {
            self.expect_get()
                .once()
                .returning(move || Ok(OsString::from(hostname.clone())));
        }

        pub fn should_not_get(&mut self, err: Errno) {
            self.expect_get().once().returning(move || Err(err));
        }
    }
}
