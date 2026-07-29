//! The runtime environment Agent Control is executing in.

use std::fmt::{self, Display, Formatter};
use strum::EnumIter;

/// The kind of host environment Agent Control is running in.
#[derive(Debug, PartialEq, Eq, Copy, Clone, EnumIter)]
pub enum Environment {
    /// A Linux host (on-host mode).
    Linux,
    /// A Windows host (on-host mode).
    Windows,
    /// A Kubernetes cluster.
    K8s,
}

impl Display for Environment {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Environment::Linux => write!(f, "linux"),
            Environment::Windows => write!(f, "windows"),
            Environment::K8s => write!(f, "kubernetes"),
        }
    }
}
