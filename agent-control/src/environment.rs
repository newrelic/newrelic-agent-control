use std::fmt::{self, Display, Formatter};

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum Environment {
    Linux,
    Windows,
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

#[cfg(test)]
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
