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

    #[cfg(target_family = "unix")]
    pub const OS_ENVIRONMENT: Environment = Environment::Linux;
    #[cfg(target_family = "windows")]
    pub const OS_ENVIRONMENT: Environment = Environment::Windows;
}
