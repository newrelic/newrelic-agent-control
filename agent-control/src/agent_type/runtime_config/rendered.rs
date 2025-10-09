use crate::agent_type::runtime_config::{k8s::K8s, on_host::rendered::OnHost};

#[derive(Debug, Clone, PartialEq)]
pub struct Runtime {
    pub deployment: Deployment,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Deployment {
    pub on_host: Option<OnHost>,
    pub k8s: Option<K8s>,
}
