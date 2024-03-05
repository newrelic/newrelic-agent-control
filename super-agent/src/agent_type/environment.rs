/// Defines the supported deployments for agent types
#[derive(Debug, PartialEq, Clone)]
pub enum Environment {
    OnHost,
    K8s,
}
