//! Default constants and capability builders shared across Agent Control: identifiers, OpAMP
//! attribute keys, OCI registry defaults, filesystem paths and store keys.

use crate::data_store::StoreKey;
use crate::opamp::remote_config::signature::SIGNATURE_CUSTOM_CAPABILITY;
use opamp_client::capabilities;
use opamp_client::opamp::proto::{AgentCapabilities, CustomCapabilities};
use opamp_client::operation::capabilities::Capabilities;

/// Reserved identifier of the Agent Control itself.
pub const AGENT_CONTROL_ID: &str = "agent-control";

/// Identifiers that cannot be used for sub-agents.
pub const RESERVED_AGENT_IDS: [&str; 1] = [AGENT_CONTROL_ID];

/// Fully-qualified Agent Control type name.
pub const AGENT_CONTROL_TYPE: &str = "com.newrelic.agent_control";
/// Namespace of the Agent Control type.
pub const AGENT_CONTROL_NAMESPACE: &str = "newrelic";
/// Agent Control version, injected at build time.
pub const AGENT_CONTROL_VERSION: &str = env!("AGENT_CONTROL_VERSION");

// Keys identifying attributes
/// OpAMP attribute key for a sub-agent chart version.
pub const OPAMP_SUBAGENT_CHART_VERSION_ATTRIBUTE_KEY: &str = "chart.version";
/// OpAMP attribute key for the Agent Control chart version.
pub const OPAMP_AC_CHART_VERSION_ATTRIBUTE_KEY: &str = "chart.version";
/// OpAMP attribute key for the continuous-delivery chart version.
pub const OPAMP_CD_CHART_VERSION_ATTRIBUTE_KEY: &str = "cd.chart.version";
/// OpAMP attribute key for the service name.
pub const OPAMP_SERVICE_NAME: &str = "service.name";
/// OpAMP attribute key for the service version.
pub const OPAMP_SERVICE_VERSION: &str = "service.version";
/// OpAMP attribute key for the service namespace.
pub const OPAMP_SERVICE_NAMESPACE: &str = "service.namespace";
/// Key name in the `agents` map of the Agent Control config.
pub const OPAMP_SUPERVISOR_KEY: &str = "supervisor.key";

/// OpAMP attribute key for the agent version.
pub const OPAMP_AGENT_VERSION_ATTRIBUTE_KEY: &str = "agent.version";

/// File name holding the agent environment variables.
pub const ENVIRONMENT_VARIABLES_FILE_NAME: &str = "environment_variables.yaml";

/// Default OCI registry host. Shared by all OCI pulls (agent packages, self-update, and the
/// agent type registry); customers override it via `oci.registry` to point at a mirror.
pub const AC_OCI_DEFAULT_REGISTRY: &str = "docker.io";

/// Default OCI repository for agent packages.
pub const AC_OCI_PACKAGE_DEFAULT_REPOSITORY: &str = "newrelic/agent-control-artifacts";
/// Public-key (JWKS) URL used to verify agent package signatures.
pub const AC_OCI_PACKAGE_PUBLIC_KEY_URL: &str =
    "https://publickeys.newrelic.com/g/agent-control-oci/global/agent-control-artifacts/jwks.json";

/// Default OCI repository for agent types.
pub const AC_OCI_AGENT_TYPES_DEFAULT_REPOSITORY: &str = "newrelic/agent-control-agent-types";
/// Public-key (JWKS) URL used to verify agent type signatures.
pub const AC_OCI_AGENT_TYPES_PUBLIC_KEY_URL: &str =
    "https://publickeys.newrelic.com/g/agent-control-oci/global/agent-type/jwks.json";

// Auth
/// File name of the authentication private key.
pub const AUTH_PRIVATE_KEY_FILE_NAME: &str = "auth_key";

// Keys non-identifying attributes
/// OpAMP attribute key for the parent agent id.
pub const PARENT_AGENT_ID_ATTRIBUTE_KEY: &str = "parent.agent.id";
/// OpAMP attribute key for the host name.
pub const HOST_NAME_ATTRIBUTE_KEY: &str = opentelemetry_semantic_conventions::attribute::HOST_NAME;
/// OpAMP attribute key for the cluster name.
pub const CLUSTER_NAME_ATTRIBUTE_KEY: &str = "cluster.name";
/// OpAMP attribute key for the host id.
pub const HOST_ID_ATTRIBUTE_KEY: &str = opentelemetry_semantic_conventions::attribute::HOST_ID;
/// OpAMP attribute key for the fleet GUID.
pub const FLEET_ID_ATTRIBUTE_KEY: &str = "fleet.guid";
/// OpAMP attribute key signalling whether external continuous delivery is enabled.
pub const CD_EXTERNAL_ENABLED_ATTRIBUTE_KEY: &str = "cd.external.enabled";
/// OpAMP attribute key signalling whether remote-update continuous delivery is enabled.
pub const CD_REMOTE_UPDATE_ENABLED_ATTRIBUTE_KEY: &str = "cd.remote_update.enabled";
/// OpAMP attribute key for the APM application id.
pub const APM_APPLICATION_ID: &str = "apm.application.id";
/// OpAMP attribute key for the execution mode.
pub const EXECUTION_MODE_ATTRIBUTE_KEY: &str = "execution.mode";

/// OpAMP attribute key for the operating-system type.
pub const OS_ATTRIBUTE_KEY: &str = "os.type";
/// Operating-system attribute value for the current target.
#[cfg(target_os = "macos")]
pub const OS_ATTRIBUTE_VALUE: &str = "darwin";
/// Operating-system attribute value for the current target.
#[cfg(target_os = "linux")]
pub const OS_ATTRIBUTE_VALUE: &str = "linux";
/// Operating-system attribute value for the current target.
#[cfg(target_os = "windows")]
pub const OS_ATTRIBUTE_VALUE: &str = "windows";

// Paths
cfg_if::cfg_if! {
    if #[cfg(target_os = "macos")] {
        /// Directory holding the local (non-remote) Agent Control data for the current target.
        pub const AGENT_CONTROL_LOCAL_DATA_DIR: &str = "/opt/homebrew/etc/newrelic-agent-control";
        /// Base data directory for the current target.
        pub const AGENT_CONTROL_DATA_DIR: &str = "/opt/homebrew/var/lib/newrelic-agent-control";
        /// Log directory for the current target.
        pub const AGENT_CONTROL_LOG_DIR: &str = "/opt/homebrew/var/log/newrelic-agent-control";
    } else if #[cfg(target_os = "windows")] {
        /// Directory holding the local (non-remote) Agent Control data for the current target.
        pub const AGENT_CONTROL_LOCAL_DATA_DIR: &str = "C:\\Program Files\\New Relic\\newrelic-agent-control";
        /// Base data directory for the current target.
        pub const AGENT_CONTROL_DATA_DIR: &str = "C:\\ProgramData\\New Relic\\newrelic-agent-control";
        /// Log directory for the current target.
        pub const AGENT_CONTROL_LOG_DIR: &str = "C:\\ProgramData\\New Relic\\newrelic-agent-control\\log";

    } else {
        /// Directory holding the local (non-remote) Agent Control data for the current target.
        pub const AGENT_CONTROL_LOCAL_DATA_DIR: &str = "/etc/newrelic-agent-control";
        /// Base data directory for the current target.
        pub const AGENT_CONTROL_DATA_DIR: &str = "/var/lib/newrelic-agent-control";
        /// Log directory for the current target.
        pub const AGENT_CONTROL_LOG_DIR: &str = "/var/log/newrelic-agent-control";
    }
}

/// - **On-host**: Used as the filename for the PID file (e.g., `newrelic-agent-control.pid`).
pub const PID_FILE_NAME: &str = "newrelic-agent-control.pid";

/// - **On-host**: Used as the directory name (e.g., `.../fleet-data/` or `.../local-data/`).
/// - **k8s**: Used as a ConfigMap prefix, followed by a hyphen (e.g., `local-data-agentid`).
pub const FOLDER_NAME_LOCAL_DATA: &str = "local-data";
/// Name used for fleet (remote) data: a directory on-host, a ConfigMap prefix on k8s.
pub const FOLDER_NAME_FLEET_DATA: &str = "fleet-data";

/// - **On-host**: Used as the base filename, combined with ".yaml" (e.g., `local_config.yaml`).
/// - **k8s**: Used as the data key within the local ConfigMap.
pub const STORE_KEY_LOCAL_DATA_CONFIG: &StoreKey = "local_config";

/// - **On-host**: Used as the base filename, combined with ".yaml" (e.g., `remote_config.yaml`).
/// - **k8s**: Used as the data key within the OpAMP/fleet ConfigMap.
pub const STORE_KEY_OPAMP_DATA_CONFIG: &StoreKey = "remote_config";
/// Store key for the persisted OpAMP instance id.
pub const STORE_KEY_INSTANCE_ID: &StoreKey = "instance_id";
/// Directory holding dynamically-fetched agent types.
pub const DYNAMIC_AGENT_TYPES_DIR: &str = "dynamic-agent-types";
/// File name holding the persisted OpAMP instance id.
pub const INSTANCE_ID_FILENAME: &str = "instance_id.yaml";
/// Folder name for an agent's managed filesystem.
pub const AGENT_FILESYSTEM_FOLDER_NAME: &str = "filesystem";
/// Folder name holding downloaded packages.
pub const PACKAGES_FOLDER_NAME: &str = "packages";
/// File name of the Agent Control log file.
pub const AGENT_CONTROL_LOG_FILENAME: &str = "newrelic-agent-control.log";
/// Suffix for per-agent stdout log files.
pub const STDOUT_LOG_FILE_NAME_SUFFIX: &str = "stdout.log";
/// Suffix for per-agent stderr log files.
pub const STDERR_LOG_FILE_NAME_SUFFIX: &str = "stderr.log";
/// Environment-variable prefix for Agent Control configuration overrides.
pub const AGENT_CONTROL_CONFIG_ENV_VAR_PREFIX: &str = "NR_AC";

/// Returns the default OpAMP [`Capabilities`] advertised by Agent Control.
pub fn default_capabilities() -> Capabilities {
    capabilities!(
        AgentCapabilities::ReportsHealth,
        AgentCapabilities::AcceptsRemoteConfig,
        AgentCapabilities::ReportsEffectiveConfig,
        AgentCapabilities::ReportsRemoteConfig,
        AgentCapabilities::ReportsStatus
    )
}

/// Returns the default OpAMP [`CustomCapabilities`] advertised by Agent Control.
pub fn default_custom_capabilities() -> CustomCapabilities {
    CustomCapabilities {
        capabilities: vec![SIGNATURE_CUSTOM_CAPABILITY.to_string()],
    }
}

/// Agent type name of the New Relic infrastructure agent.
pub const AGENT_TYPE_NAME_INFRA_AGENT: &str = "com.newrelic.infrastructure";
/// Agent type name of the New Relic OpenTelemetry collector (NRDOT).
pub const AGENT_TYPE_NAME_NRDOT: &str = "com.newrelic.opentelemetry.collector";

// Fleet Control auto generated agent id
/// Fleet-Control auto-generated agent id for the infrastructure agent.
pub const AGENT_ID_INFRA_AGENT: &str = "nr-infra";
/// Fleet-Control auto-generated agent id for NRDOT.
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
