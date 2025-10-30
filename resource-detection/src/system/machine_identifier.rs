#[cfg_attr(unix, path = "machine_identifier/unix.rs")]
#[cfg_attr(windows, path = "machine_identifier/windows.rs")]
mod identifier;

pub(crate) use identifier::MachineIdentityProvider;
