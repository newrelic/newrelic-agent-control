pub mod ebpf_agent;
pub mod infra_agent;
pub mod nrdot_agent;
pub mod proxy;
pub mod remote_config;

// TODO we should get the version dynamically from the recipe itself
const INFRA_AGENT_VERSION: &str = "1.72.1";
