#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(crate) use unix::MachineIdentityProvider;
#[cfg(windows)]
pub(crate) use windows::MachineIdentityProvider;
