use opamp_client::capabilities;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::capabilities::Capabilities;
use opamp_client::operation::settings::DescriptionValueType;

pub static SUPER_AGENT_ID: &str = "super-agent";
pub static SUPER_AGENT_TYPE: &str = "com.newrelic.super_agent";
pub static SUPER_AGENT_NAMESPACE: &str = "newrelic";
pub static SUPER_AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub static NEWRELIC_INFRA_AGENT_VERSION: &str =
    konst::option::unwrap_or!(option_env!("NEWRELIC_INFRA_AGENT_VERSION"), "0.0.0");
pub static NR_OTEL_COLLECTOR_VERSION: &str =
    konst::option::unwrap_or!(option_env!("NR_OTEL_COLLECTOR_VERSION"), "0.0.0");

// Keys identifying attributes
pub static OPAMP_SERVICE_NAME: &str = "service.name";
pub static OPAMP_SERVICE_VERSION: &str = "service.version";
pub static OPAMP_SERVICE_NAMESPACE: &str = "service.namespace";
pub static OPAMP_AGENT_VERSION_ATTRIBUTE_KEY: &str = "agent.version";

// Auth
pub static AUTH_PRIVATE_KEY_FILE_NAME: &str = "auth_key";

// Keys non-identifying attributes
pub static PARENT_AGENT_ID_ATTRIBUTE_KEY: &str = "parent.agent.id";
pub static HOST_NAME_ATTRIBUTE_KEY: &str = opentelemetry_semantic_conventions::resource::HOST_NAME;
pub static CLUSTER_NAME_ATTRIBUTE_KEY: &str = "cluster.name";
pub static HOST_ID_ATTRIBUTE_KEY: &str = opentelemetry_semantic_conventions::resource::HOST_ID;
pub static FLEET_ID_ATTRIBUTE_KEY: &str = "fleet.guid";

// Paths
// TODO: should we rename SUPER_AGENT_DATA_DIR to SUPER_AGENT_REMOTE_DATA_DIR?
cfg_if::cfg_if! {
    if #[cfg(target_os = "macos")] {
        pub static SUPER_AGENT_LOCAL_DATA_DIR: &str = "/opt/homebrew/etc/newrelic-super-agent";
        pub static SUPER_AGENT_DATA_DIR: &str = "/opt/homebrew/var/lib/newrelic-super-agent";
        pub static SUPER_AGENT_LOG_DIR: &str = "/opt/homebrew/var/log/newrelic-super-agent";

    }else{
        pub static SUPER_AGENT_LOCAL_DATA_DIR: &str = "/etc/newrelic-super-agent";
        pub static SUPER_AGENT_DATA_DIR: &str = "/var/lib/newrelic-super-agent";
        pub static SUPER_AGENT_LOG_DIR: &str = "/var/log/newrelic-super-agent";
    }
}

pub static SUB_AGENT_DIR: &str = "fleet/agents.d";
pub static SUPER_AGENT_CONFIG_FILE: &str = "config.yaml";
pub static DYNAMIC_AGENT_TYPE_FILENAME: &str = "dynamic-agent-type.yaml";
pub static IDENTIFIERS_FILENAME: &str = "identifiers.yaml";
pub static VALUES_DIR: &str = "values";
pub static VALUES_FILE: &str = "values.yaml";
pub static GENERATED_FOLDER_NAME: &str = "auto-generated";
pub static SUPER_AGENT_LOG_FILENAME: &str = "newrelic-super-agent.log";
pub static STDOUT_LOG_PREFIX: &str = "stdout.log";
pub static STDERR_LOG_PREFIX: &str = "stderr.log";
pub static SUPER_AGENT_CONFIG_ENV_VAR_PREFIX: &str = "NR_SA";

pub fn default_capabilities() -> Capabilities {
    capabilities!(
        AgentCapabilities::ReportsHealth,
        AgentCapabilities::AcceptsRemoteConfig,
        AgentCapabilities::ReportsEffectiveConfig,
        AgentCapabilities::ReportsRemoteConfig,
        AgentCapabilities::ReportsStatus
    )
}

pub const FQN_NAME_INFRA_AGENT: &str = "com.newrelic.infrastructure_agent";
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
