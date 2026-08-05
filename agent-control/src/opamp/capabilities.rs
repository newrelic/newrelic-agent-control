//! Identifiers for OpAMP custom capabilities advertised by Agent Control.
use opamp_client::opamp::proto::CustomCapabilities as ProtoCustomCapabilities;
use std::fmt;

/// Custom capability identifiers advertised via OpAMP `CustomCapabilities`/`CustomMessage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomCapability {
    /// Support for remote config signature verification.
    Signature,
    /// The configured Agent Type OCI repository was reachable at startup.
    RemoteAgentTypeRepoReachable,
    /// This Agent Control is not managed by an agent-control-cd deployment.
    K8sConfigOnlyAgents,
}

impl CustomCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Signature => "com.newrelic.security.configSignature",
            Self::RemoteAgentTypeRepoReachable => "com.newrelic.remoteAgentTypeRepoReachable",
            Self::K8sConfigOnlyAgents => "com.newrelic.k8s_config_only_agents",
        }
    }
}

impl fmt::Display for CustomCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A list of custom capabilities advertised via OpAMP, convertible to the proto wire type.
#[derive(Debug, Clone, Default)]
pub struct CustomCapabilities(Vec<CustomCapability>);

impl CustomCapabilities {
    pub fn push(&mut self, capability: CustomCapability) {
        self.0.push(capability);
    }
    pub fn extend_from_slice(&mut self, capabilities: &[CustomCapability]) {
        self.0.extend_from_slice(capabilities);
    }
}

impl From<Vec<CustomCapability>> for CustomCapabilities {
    fn from(capabilities: Vec<CustomCapability>) -> Self {
        Self(capabilities)
    }
}

impl From<CustomCapabilities> for ProtoCustomCapabilities {
    fn from(capabilities: CustomCapabilities) -> Self {
        Self {
            capabilities: capabilities
                .0
                .iter()
                .map(CustomCapability::to_string)
                .collect(),
        }
    }
}
