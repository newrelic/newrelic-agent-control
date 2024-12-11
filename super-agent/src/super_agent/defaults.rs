use opamp_client::capabilities;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::capabilities::Capabilities;
use opamp_client::operation::settings::DescriptionValueType;

pub const SUPER_AGENT_ID: &str = "super-agent";
pub const SUPER_AGENT_TYPE: &str = "com.newrelic.super_agent";
pub const SUPER_AGENT_NAMESPACE: &str = "newrelic";
pub const SUPER_AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const NEWRELIC_INFRA_AGENT_VERSION: &str =
    konst::option::unwrap_or!(option_env!("NEWRELIC_INFRA_AGENT_VERSION"), "0.0.0");
pub const NR_OTEL_COLLECTOR_VERSION: &str =
    konst::option::unwrap_or!(option_env!("NR_OTEL_COLLECTOR_VERSION"), "0.0.0");

// Keys identifying attributes
pub const OPAMP_SERVICE_NAME: &str = "service.name";
pub const OPAMP_SERVICE_VERSION: &str = "service.version";
pub const OPAMP_SERVICE_NAMESPACE: &str = "service.namespace";
pub const OPAMP_AGENT_VERSION_ATTRIBUTE_KEY: &str = "agent.version";

// Auth
pub const AUTH_PRIVATE_KEY_FILE_NAME: &str = "auth_key";

// Keys non-identifying attributes
pub const PARENT_AGENT_ID_ATTRIBUTE_KEY: &str = "parent.agent.id";
pub const HOST_NAME_ATTRIBUTE_KEY: &str = opentelemetry_semantic_conventions::resource::HOST_NAME;
pub const CLUSTER_NAME_ATTRIBUTE_KEY: &str = "cluster.name";
pub const HOST_ID_ATTRIBUTE_KEY: &str = opentelemetry_semantic_conventions::resource::HOST_ID;
pub const FLEET_ID_ATTRIBUTE_KEY: &str = "fleet.guid";

// Paths
// TODO: should we rename SUPER_AGENT_DATA_DIR to SUPER_AGENT_REMOTE_DATA_DIR?
cfg_if::cfg_if! {
    if #[cfg(target_os = "macos")] {
        pub const SUPER_AGENT_LOCAL_DATA_DIR: &str = "/opt/homebrew/etc/newrelic-super-agent";
        pub const SUPER_AGENT_DATA_DIR: &str = "/opt/homebrew/var/lib/newrelic-super-agent";
        pub const SUPER_AGENT_LOG_DIR: &str = "/opt/homebrew/var/log/newrelic-super-agent";

    }else{
        pub const SUPER_AGENT_LOCAL_DATA_DIR: &str = "/etc/newrelic-super-agent";
        pub const SUPER_AGENT_DATA_DIR: &str = "/var/lib/newrelic-super-agent";
        pub const SUPER_AGENT_LOG_DIR: &str = "/var/log/newrelic-super-agent";
    }
}

pub const SUB_AGENT_DIR: &str = "fleet/agents.d";
pub const SUPER_AGENT_CONFIG_FILE: &str = "config.yaml";
pub const DYNAMIC_AGENT_TYPE_FILENAME: &str = "dynamic-agent-type.yaml";
pub const IDENTIFIERS_FILENAME: &str = "identifiers.yaml";
pub const VALUES_DIR: &str = "values";
pub const VALUES_FILE: &str = "values.yaml";
pub const GENERATED_FOLDER_NAME: &str = "auto-generated";
pub const SUPER_AGENT_LOG_FILENAME: &str = "newrelic-super-agent.log";
pub const STDOUT_LOG_PREFIX: &str = "stdout.log";
pub const STDERR_LOG_PREFIX: &str = "stderr.log";
pub const SUPER_AGENT_CONFIG_ENV_VAR_PREFIX: &str = "NR_SA";

pub fn default_capabilities() -> Capabilities {
    capabilities!(
        AgentCapabilities::ReportsHealth,
        AgentCapabilities::AcceptsRemoteConfig,
        AgentCapabilities::ReportsEffectiveConfig,
        AgentCapabilities::ReportsRemoteConfig,
        AgentCapabilities::ReportsStatus
    )
}

pub const FQN_NAME_INFRA_AGENT: &str = "com.newrelic.infrastructure";
pub const FQN_NAME_NRDOT: &str = "io.opentelemetry.collector";

pub fn sub_agent_version(agent_type: &str) -> Option<DescriptionValueType> {
    match agent_type {
        FQN_NAME_INFRA_AGENT => Some(DescriptionValueType::String(
            NEWRELIC_INFRA_AGENT_VERSION.to_string(),
        )),
        FQN_NAME_NRDOT => Some(DescriptionValueType::String(
            NR_OTEL_COLLECTOR_VERSION.to_string(),
        )),
        _ => None,
    }
}
