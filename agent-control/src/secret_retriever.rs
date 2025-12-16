use crate::agent_control::config::OpAMPClientConfig;

pub mod k8s;
pub mod on_host;

pub trait OpampSecretRetriever {
    type Error: std::error::Error;
    fn retrieve(&self, opamp_config: &OpAMPClientConfig) -> Result<String, Self::Error>;
}
