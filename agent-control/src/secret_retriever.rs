use crate::agent_control::config::OpAMPClientConfig;
use crate::agent_control::run::RunError;

pub mod k8s;
pub mod on_host;
pub trait OpampSecretRetriever {
    fn retrieve(&self, opamp_config: &OpAMPClientConfig) -> Result<String, RunError>;
}
