use crate::agent_type::agent_type_id::AgentTypeID;
use crate::data_store::StoreKey;
use crate::opamp::remote_config::signature::SIGNATURE_CUSTOM_CAPABILITY;
use crate::sub_agent::identity::AgentIdentity;
use opamp_client::capabilities;
use opamp_client::opamp::proto::{AgentCapabilities, CustomCapabilities};
use opamp_client::operation::capabilities::Capabilities;

pub const AGENT_CONTROL_ID: &str = "agent-control";

pub const RESERVED_AGENT_IDS: [&str; 1] = [AGENT_CONTROL_ID];

pub const AGENT_CONTROL_TYPE: &str = "com.newrelic.agent_control";
pub const AGENT_CONTROL_NAMESPACE: &str = "newrelic";
pub const AGENT_CONTROL_VERSION: &str = env!("CARGO_PKG_VERSION");

// Keys identifying attributes
pub const OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY: &str = "chart.version";
pub const OPAMP_AC_CHART_VERSION_ATTRIBUTE_KEY: &str = "chart.version"; // TODO `ac.chart.version`?
pub const OPAMP_CD_CHART_VERSION_ATTRIBUTE_KEY: &str = "cd.chart.version";
pub const OPAMP_SERVICE_NAME: &str = "service.name";
pub const OPAMP_SERVICE_VERSION: &str = "service.version";
pub const OPAMP_SERVICE_NAMESPACE: &str = "service.namespace";
// This is the key name in the agents{} map of the AC config
pub const OPAMP_SUPERVISOR_KEY: &str = "supervisor.key";

pub const OPAMP_AGENT_VERSION_ATTRIBUTE_KEY: &str = "agent.version";

// Auth
pub const AUTH_PRIVATE_KEY_FILE_NAME: &str = "auth_key";

// Keys non-identifying attributes
pub const PARENT_AGENT_ID_ATTRIBUTE_KEY: &str = "parent.agent.id";
pub const HOST_NAME_ATTRIBUTE_KEY: &str = opentelemetry_semantic_conventions::attribute::HOST_NAME;
pub const CLUSTER_NAME_ATTRIBUTE_KEY: &str = "cluster.name";
pub const HOST_ID_ATTRIBUTE_KEY: &str = opentelemetry_semantic_conventions::attribute::HOST_ID;
pub const FLEET_ID_ATTRIBUTE_KEY: &str = "fleet.guid";
pub const CD_EXTERNAL_ENABLED_ATTRIBUTE_KEY: &str = "cd.external.enabled";
pub const CD_REMOTE_UPDATE_ENABLED_ATTRIBUTE_KEY: &str = "cd.remote_update.enabled";

pub const OS_ATTRIBUTE_KEY: &str = "os.type";
#[cfg(target_os = "macos")]
pub const OS_ATTRIBUTE_VALUE: &str = "darwin";
#[cfg(target_os = "linux")]
pub const OS_ATTRIBUTE_VALUE: &str = "linux";
#[cfg(target_os = "windows")]
pub const OS_ATTRIBUTE_VALUE: &str = "windows";

// Paths
// TODO: should we rename AGENT_CONTROL_DATA_DIR to AGENT_CONTROL_REMOTE_DATA_DIR?
cfg_if::cfg_if! {
    if #[cfg(target_os = "macos")] {
        pub const AGENT_CONTROL_LOCAL_DATA_DIR: &str = "/opt/homebrew/etc/newrelic-agent-control";
        pub const AGENT_CONTROL_DATA_DIR: &str = "/opt/homebrew/var/lib/newrelic-agent-control";
        pub const AGENT_CONTROL_LOG_DIR: &str = "/opt/homebrew/var/log/newrelic-agent-control";

    } else if #[cfg(target_os = "windows")] {
        pub const AGENT_CONTROL_LOCAL_DATA_DIR: &str = "C:\\Program Files\\New Relic\\newrelic-agent-control";
        pub const AGENT_CONTROL_DATA_DIR: &str = "C:\\ProgramData\\New Relic\\newrelic-agent-control";
        pub const AGENT_CONTROL_LOG_DIR: &str = "C:\\ProgramData\\New Relic\\newrelic-agent-control\\logs";

    } else {
        pub const AGENT_CONTROL_LOCAL_DATA_DIR: &str = "/etc/newrelic-agent-control";
        pub const AGENT_CONTROL_DATA_DIR: &str = "/var/lib/newrelic-agent-control";
        pub const AGENT_CONTROL_LOG_DIR: &str = "/var/log/newrelic-agent-control";
    }
}

/// - **On-host**: Used as the directory name (e.g., `.../fleet-data/` or `.../local-data/`).
/// - **k8s**: Used as a ConfigMap prefix, followed by a hyphen (e.g., `local-data-agentid`).
pub const FOLDER_NAME_LOCAL_DATA: &str = "local-data";
pub const FOLDER_NAME_FLEET_DATA: &str = "fleet-data";

/// - **On-host**: Used as the base filename, combined with ".yaml" (e.g., `local_config.yaml`).
/// - **k8s**: Used as the data key within the local ConfigMap.
pub const STORE_KEY_LOCAL_DATA_CONFIG: &StoreKey = "local_config";

/// - **On-host**: Used as the base filename, combined with ".yaml" (e.g., `remote_config.yaml`).
/// - **k8s**: Used as the data key within the OpAMP/fleet ConfigMap.
pub const STORE_KEY_OPAMP_DATA_CONFIG: &StoreKey = "remote_config";
pub const STORE_KEY_INSTANCE_ID: &StoreKey = "instance_id";
pub const DYNAMIC_AGENT_TYPE_DIR: &str = "dynamic-agent-types";
pub const INSTANCE_ID_FILENAME: &str = "instance_id.yaml";
pub const GENERATED_FOLDER_NAME: &str = "auto-generated";
pub const AGENT_CONTROL_LOG_FILENAME: &str = "newrelic-agent-control.log";
pub const STDOUT_LOG_PREFIX: &str = "stdout.log";
pub const STDERR_LOG_PREFIX: &str = "stderr.log";
pub const AGENT_CONTROL_CONFIG_ENV_VAR_PREFIX: &str = "NR_AC";

pub fn default_capabilities() -> Capabilities {
    capabilities!(
        AgentCapabilities::ReportsHealth,
        AgentCapabilities::AcceptsRemoteConfig,
        AgentCapabilities::ReportsEffectiveConfig,
        AgentCapabilities::ReportsRemoteConfig,
        AgentCapabilities::ReportsStatus
    )
}

pub fn default_sub_agent_custom_capabilities() -> CustomCapabilities {
    CustomCapabilities {
        capabilities: vec![SIGNATURE_CUSTOM_CAPABILITY.to_string()],
    }
}

pub(crate) fn get_custom_capabilities(agent_type_id: &AgentTypeID) -> Option<CustomCapabilities> {
    if agent_type_id.eq(&AgentIdentity::new_agent_control_identity().agent_type_id) {
        // Agent_Control does not have custom capabilities for now
        return None;
    }

    Some(default_sub_agent_custom_capabilities())
}

pub const AGENT_TYPE_NAME_INFRA_AGENT: &str = "com.newrelic.infrastructure";
pub const AGENT_TYPE_NAME_NRDOT: &str = "com.newrelic.opentelemetry.collector";

// Fleet Control auto generated agent id
pub const AGENT_ID_INFRA_AGENT: &str = "nr-infra";
pub const AGENT_ID_NRDOT: &str = "nrdot";

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn test_otel_semconv_experimental_fields() {
        // Detect possible breaking changes when upgrading opentelemetry-semantic-conventions
        assert_eq!(HOST_NAME_ATTRIBUTE_KEY, "host.name");
        assert_eq!(HOST_ID_ATTRIBUTE_KEY, "host.id");
    }
}
