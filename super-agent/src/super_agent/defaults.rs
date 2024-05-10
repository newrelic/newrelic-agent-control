use opamp_client::capabilities;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::capabilities::Capabilities;
use paste::paste;
use std::sync::OnceLock;

// What does this do?
// This macro generates a static variable and a function that returns a reference to that variable.
// Given an identifier `SUPER_AGENT_ID` and a value `"super-agent"` of type `&str`, the macro call
// `generate_const_getter!(SUPER_AGENT_ID, "super-agent")` generates the following:
//
// ```
// static SUPER_AGENT_ID_STATIC: OnceLock<&str> = OnceLock::new();
//
// pub(crate) fn super_agent_id() -> &'static str {
//     SUPER_AGENT_ID_STATIC.get_or_init(|| "super-agent")
// }
// ```
//
// The `OnceLock` type is a wrapper around `std::sync::Once` that allows for the initialization of
// a static variable with a closure that returns the value to be stored. The `get_or_init` method
// ensures that the closure is only called once, and the value is stored and returned for subsequent
// calls.
macro_rules! generate_const_getter {
    ($name:ident, $value:expr) => {
        paste! {
            static [<$name:upper _STATIC>]: OnceLock<&str> = OnceLock::new();

            // I want this function usage to be analogous to referencing a constant, hence uppercase
            #[allow(non_snake_case)]
            pub fn [<$name:upper>]() -> &'static str {
                [<$name:upper _STATIC>].get_or_init(|| $value)
            }
        }
    };
}

generate_const_getter!(SUPER_AGENT_ID, "super-agent");
generate_const_getter!(SUPER_AGENT_TYPE, "com.newrelic.super_agent");
generate_const_getter!(SUPER_AGENT_NAMESPACE, "newrelic");
generate_const_getter!(SUPER_AGENT_VERSION, env!("CARGO_PKG_VERSION"));

// Keys non-identifying attributes
generate_const_getter!(PARENT_AGENT_ID_ATTRIBUTE_KEY, "parent.agent.id");
generate_const_getter!(
    HOST_NAME_ATTRIBUTE_KEY,
    opentelemetry_semantic_conventions::resource::HOST_NAME
);
generate_const_getter!(CLUSTER_NAME_ATTRIBUTE_KEY, "cluster.name");
generate_const_getter!(
    HOST_ID_ATTRIBUTE_KEY,
    opentelemetry_semantic_conventions::resource::HOST_ID
);
generate_const_getter!(FLEET_ID_ATTRIBUTE_KEY, "fleet.guid");

// Paths
cfg_if::cfg_if! {
    if #[cfg(target_os = "macos")] {
        generate_const_getter!(SUPER_AGENT_LOCAL_DATA_DIR, "/opt/homebrew/etc/newrelic-super-agent");
        generate_const_getter!(SUPER_AGENT_IDENTIFIERS_PATH, "/opt/homebrew/var/lib/newrelic-super-agent/identifiers.yaml");
        generate_const_getter!(REMOTE_AGENT_DATA_DIR, "/opt/homebrew/var/lib/newrelic-super-agent/fleet/agents.d");
        generate_const_getter!(LOCAL_AGENT_DATA_DIR, "/opt/homebrew/etc/newrelic-super-agent/fleet/agents.d");

        generate_const_getter!(SUPER_AGENT_DATA_DIR, "/opt/homebrew/var/lib/newrelic-super-agent");
        generate_const_getter!(DYNAMIC_AGENT_TYPE, "/opt/homebrew/etc/newrelic-super-agent/dynamic-agent-type.yaml");

        // Logging constants
        generate_const_getter!(SUPER_AGENT_LOG_DIR, "/opt/homebrew/var/log/newrelic-super-agent");
        generate_const_getter!(SUB_AGENT_LOG_DIR, "/opt/homebrew/var/log/newrelic-super-agent/fleet/agents.d");

    }else{
        generate_const_getter!(SUPER_AGENT_LOCAL_DATA_DIR, "/etc/newrelic-super-agent");
        generate_const_getter!(SUPER_AGENT_IDENTIFIERS_PATH, "/var/lib/newrelic-super-agent/identifiers.yaml");
        generate_const_getter!(REMOTE_AGENT_DATA_DIR, "/var/lib/newrelic-super-agent/fleet/agents.d");
        generate_const_getter!(LOCAL_AGENT_DATA_DIR, "/etc/newrelic-super-agent/fleet/agents.d");

        generate_const_getter!(SUPER_AGENT_DATA_DIR, "/var/lib/newrelic-super-agent");
        generate_const_getter!(DYNAMIC_AGENT_TYPE, "/etc/newrelic-super-agent/dynamic-agent-type.yaml");

        // Logging constants
        generate_const_getter!(SUPER_AGENT_LOG_DIR, "/var/log/newrelic-super-agent");
        generate_const_getter!(SUB_AGENT_LOG_DIR, "/var/log/newrelic-super-agent/fleet/agents.d");
    }
}

generate_const_getter!(SUB_AGENT_DIRECTORY, "agents.d");
generate_const_getter!(VALUES_DIR, "values");
generate_const_getter!(VALUES_FILE, "values.yaml");
generate_const_getter!(GENERATED_FOLDER_NAME, "auto-generated");
generate_const_getter!(SUPER_AGENT_LOG_FILENAME, "newrelic-super-agent.log");
generate_const_getter!(STDOUT_LOG_PREFIX, "stdout.log");
generate_const_getter!(STDERR_LOG_PREFIX, "stderr.log");

pub fn debug_mode() {
    // Sets some of the variables to a "debug" value for easy testing!
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
