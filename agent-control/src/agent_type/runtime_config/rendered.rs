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

#[cfg(test)]
impl Deployment {
    pub fn on_host(self) -> OnHost {
        match self {
            Self::Host(on_host) => on_host,
            Self::K8s(_) => unreachable!("expected host deployment"),
        }
    }

    pub fn k8s(self) -> K8s {
        match self {
            Self::K8s(k8s) => k8s,
            Self::Host(_) => unreachable!("expected k8s deployment"),
        }
    }
}
