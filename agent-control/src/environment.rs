//! The runtime environment Agent Control is executing in.

use std::fmt::{self, Display, Formatter};

/// The kind of host environment Agent Control is running in.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
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

impl Environment {
    /// Coarser-grained deployment platform: "host" for both on-host environments (the real OS
    /// is already available separately via os.type) vs "kubernetes". Used for self-instrumentation
    /// telemetry, where the linux/windows split adds noise without adding information.
    pub fn deployment_platform(&self) -> &'static str {
        match self {
            Environment::Linux | Environment::Windows => "host",
            Environment::K8s => "kubernetes",
        }
    }
}

#[cfg(test)]
#[allow(missing_docs)]
pub mod tests {
    use super::*;

    impl Environment {
        /// Iterates over every [Environment] variant.
        pub fn all() -> impl Iterator<Item = Environment> {
            std::iter::successors(Some(Environment::Linux), Environment::succeeding)
        }

        fn succeeding(current: &Environment) -> Option<Environment> {
            match current {
                Environment::Linux => Some(Environment::Windows),
                Environment::Windows => Some(Environment::K8s),
                Environment::K8s => None,
            }
        }
    }
}
