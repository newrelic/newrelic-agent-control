use crate::agent_type::runtime_config::{k8s::K8s, on_host::rendered::OnHost};

/// The runtime definition of an agent type after it has been completely
/// rendered by the templating process.
///
/// This is used by the supervisorss to know how to create and manage the workload resources for
/// the agent.
#[derive(Debug, Clone, PartialEq)]
pub struct Runtime {
    pub deployment: Deployment,
}

/// Deployment definition for an agent type after it has been completely rendered by the templating
/// process.
///
/// Specifies if there are `on_host` instructions, `k8s` instructions, both or none.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Deployment {
    pub on_host: Option<OnHost>,
    pub k8s: Option<K8s>,
}
