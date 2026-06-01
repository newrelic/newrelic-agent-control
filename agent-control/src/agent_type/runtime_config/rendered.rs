use crate::agent_type::runtime_config::{k8s::K8s, on_host::rendered::OnHost};

/// The runtime definition of an agent type after it has been completely rendered by the
/// templating process.
#[derive(Debug, Clone, PartialEq)]
pub struct Runtime {
    pub deployment: Deployment,
}

/// Deployment definition for an agent type after templating.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Deployment {
    Host(OnHost),
    K8s(K8s),
}
