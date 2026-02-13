pub mod installation_infra_agent;
pub mod installation_nrdot;
pub mod proxy;
pub mod service_wrong_config;

// TODO we should get the version dynamically from the recipe itself
const INFRA_AGENT_VERSION: &str = "1.72.1";
const NRDOT_VERSION: &str = "1.8.0";

const DEFAULT_STATUS_PORT: u16 = 51200;
