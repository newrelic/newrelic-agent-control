use opamp_client::capabilities;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::capabilities::Capabilities;

pub const SUPER_AGENT_ID: &str = "super-agent";
pub const SUPER_AGENT_TYPE: &str = "com.newrelic.super_agent";
pub const SUPER_AGENT_NAMESPACE: &str = "newrelic";
pub const SUPER_AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

// Keys non-identifying attributes
pub const PARENT_AGENT_ID_ATTRIBUTE_KEY: &str = "parent.agent.id";
pub const HOST_NAME_ATTRIBUTE_KEY: &str = opentelemetry_semantic_conventions::resource::HOST_NAME;
pub const CLUSTER_NAME_ATTRIBUTE_KEY: &str = "cluster.name";
pub const HOST_ID_ATTRIBUTE_KEY: &str = opentelemetry_semantic_conventions::resource::HOST_ID;

// Paths
cfg_if::cfg_if! {
    if #[cfg(target_os = "macos")] {
        pub const SUB_AGENT_DIRECTORY: &str = "agents.d";
        pub const SUPER_AGENT_LOCAL_DATA_DIR: &str = "/opt/homebrew/etc/newrelic-super-agent";
        pub const SUPER_AGENT_IDENTIFIERS_PATH: &str = "/opt/homebrew/var/lib/newrelic-super-agent/identifiers.yaml";
        pub const REMOTE_AGENT_DATA_DIR: &str = "/opt/homebrew/var/lib/newrelic-super-agent/fleet/agents.d";
        pub const LOCAL_AGENT_DATA_DIR: &str = "/opt/homebrew/etc/newrelic-super-agent/fleet/agents.d";
        pub const VALUES_DIR: &str = "values";
        pub const VALUES_FILE: &str = "values.yaml";
        pub const SUPER_AGENT_DATA_DIR: &str = "/opt/homebrew/var/lib/newrelic-super-agent";
        pub const GENERATED_FOLDER_NAME: &str = "auto-generated";
        pub const DYNAMIC_AGENT_TYPE :&str = "/opt/homebrew/etc/newrelic-super-agent/dynamic-agent-type.yaml";

        // Logging constants
        pub const SUPER_AGENT_LOG_DIR: &str = "/opt/homebrew/var/log/newrelic-super-agent";
        pub const SUPER_AGENT_LOG_FILENAME: &str = "newrelic-super-agent.log";
        pub const SUB_AGENT_LOG_DIR: &str = "/opt/homebrew/var/log/newrelic-super-agent/fleet/agents.d";
        pub const STDOUT_LOG_PREFIX: &str = "stdout.log";
        pub const STDERR_LOG_PREFIX: &str = "stderr.log";
    }else{
        pub const SUB_AGENT_DIRECTORY: &str = "agents.d";
        pub const SUPER_AGENT_LOCAL_DATA_DIR: &str = "/etc/newrelic-super-agent";
        pub const SUPER_AGENT_IDENTIFIERS_PATH: &str = "/var/lib/newrelic-super-agent/identifiers.yaml";
        pub const REMOTE_AGENT_DATA_DIR: &str = "/var/lib/newrelic-super-agent/fleet/agents.d";
        pub const LOCAL_AGENT_DATA_DIR: &str = "/etc/newrelic-super-agent/fleet/agents.d";
        pub const VALUES_DIR: &str = "values";
        pub const VALUES_FILE: &str = "values.yaml";
        pub const SUPER_AGENT_DATA_DIR: &str = "/var/lib/newrelic-super-agent";
        pub const GENERATED_FOLDER_NAME: &str = "auto-generated";
        pub const DYNAMIC_AGENT_TYPE :&str = "/etc/newrelic-super-agent/dynamic-agent-type.yaml";

        // Logging constants
        pub const SUPER_AGENT_LOG_DIR: &str = "/var/log/newrelic-super-agent";
        pub const SUPER_AGENT_LOG_FILENAME: &str = "newrelic-super-agent.log";
        pub const SUB_AGENT_LOG_DIR: &str = "/var/log/newrelic-super-agent/fleet/agents.d";
        pub const STDOUT_LOG_PREFIX: &str = "stdout.log";
        pub const STDERR_LOG_PREFIX: &str = "stderr.log";
    }
}

pub fn default_capabilities() -> Capabilities {
    capabilities!(
        AgentCapabilities::ReportsHealth,
        AgentCapabilities::AcceptsRemoteConfig,
        AgentCapabilities::ReportsEffectiveConfig,
        AgentCapabilities::ReportsRemoteConfig,
        AgentCapabilities::ReportsStatus
    )
}
