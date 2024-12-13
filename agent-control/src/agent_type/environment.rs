use std::fmt;

/// Defines the supported deployments for agent types
#[derive(Debug, PartialEq, Clone)]
pub enum Environment {
    OnHost,
    K8s,
}
impl fmt::Display for Environment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Environment::OnHost => write!(f, "on_host"),
            Environment::K8s => write!(f, "k8s"),
        }
    }
}
