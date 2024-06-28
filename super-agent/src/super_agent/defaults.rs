use opamp_client::capabilities;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::capabilities::Capabilities;
use paste::paste;
#[cfg(debug_assertions)]
use std::path::Path;
use std::sync::OnceLock;
#[cfg(debug_assertions)]
use tracing::debug;

use std::sync::Mutex;

// What does this do?
// This macro generates a static variable and a function that returns a reference to that variable.
// This allows to set this variables on initialization, like what is done when using the super-agent
// `--debug` flag.
// Given an identifier `SUPER_AGENT_ID` and a value `"super-agent"` of type `&str`, the macro call
// `generate_const_getter!(SUPER_AGENT_ID, "super-agent")` generates the following:
//
// ```
// static SUPER_AGENT_ID_STATIC: OnceLock<&str> = OnceLock::new();
//
// #[allow(non_snake_case)]
// pub(crate) fn SUPER_AGENT_ID() -> &'static str {
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
            static [<$name:upper _STATIC>]: OnceLock<Mutex<String>> = OnceLock::new();

            // I want this function usage to be analogous to referencing a constant, hence uppercase
            #[allow(non_snake_case)]
            pub fn [<$name:upper>]() -> String {
                [<$name:upper _STATIC>].get_or_init(|| Mutex::new($value.to_string())).lock().unwrap().clone()
            }
        }
    };
}

generate_const_getter!(SUPER_AGENT_ID, "super-agent");
generate_const_getter!(SUPER_AGENT_TYPE, "com.newrelic.super_agent");
generate_const_getter!(SUPER_AGENT_NAMESPACE, "newrelic");
generate_const_getter!(SUPER_AGENT_VERSION, env!("CARGO_PKG_VERSION"));

// Auth
generate_const_getter!(AUTH_PRIVATE_KEY_FILE_NAME, "auth_key");

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
        generate_const_getter!(SUPER_AGENT_DATA_DIR, "/opt/homebrew/var/lib/newrelic-super-agent");
        generate_const_getter!(SUPER_AGENT_LOG_DIR, "/opt/homebrew/var/log/newrelic-super-agent");

    }else{
        generate_const_getter!(SUPER_AGENT_LOCAL_DATA_DIR, "/etc/newrelic-super-agent");
        generate_const_getter!(SUPER_AGENT_DATA_DIR, "/var/lib/newrelic-super-agent");
        generate_const_getter!(SUPER_AGENT_LOG_DIR, "/var/log/newrelic-super-agent");
    }
}

generate_const_getter!(
    LOCAL_AGENT_DATA_DIR,
    format!(
        "{}/{}/{}",
        SUPER_AGENT_LOCAL_DATA_DIR(),
        FLEET_DIR(),
        SUB_AGENT_DIRECTORY()
    )
);
generate_const_getter!(
    DYNAMIC_AGENT_TYPE,
    format!(
        "{}/{}",
        SUPER_AGENT_LOCAL_DATA_DIR(),
        DYNAMIC_AGENT_TYPE_FILENAME()
    )
);
generate_const_getter!(
    SUPER_AGENT_IDENTIFIERS_PATH,
    format!("{}/{}", SUPER_AGENT_DATA_DIR(), IDENTIFIERS_FILENAME()).as_str()
);
generate_const_getter!(
    REMOTE_AGENT_DATA_DIR,
    format!(
        "{}/{}/{}",
        SUPER_AGENT_DATA_DIR(),
        FLEET_DIR(),
        SUB_AGENT_DIRECTORY()
    )
);
generate_const_getter!(
    SUB_AGENT_LOG_DIR,
    format!(
        "{}/{}/{}",
        SUPER_AGENT_LOG_DIR(),
        FLEET_DIR(),
        SUB_AGENT_DIRECTORY()
    )
);
generate_const_getter!(DYNAMIC_AGENT_TYPE_FILENAME, "dynamic-agent-type.yaml");
generate_const_getter!(IDENTIFIERS_FILENAME, "identifiers.yaml");
generate_const_getter!(FLEET_DIR, "fleet");
generate_const_getter!(SUB_AGENT_DIRECTORY, "agents.d");
generate_const_getter!(VALUES_DIR, "values");
generate_const_getter!(VALUES_FILE, "values.yaml");
generate_const_getter!(GENERATED_FOLDER_NAME, "auto-generated");
generate_const_getter!(SUPER_AGENT_LOG_FILENAME, "newrelic-super-agent.log");
generate_const_getter!(STDOUT_LOG_PREFIX, "stdout.log");
generate_const_getter!(STDERR_LOG_PREFIX, "stderr.log");

#[cfg(debug_assertions)]
pub fn set_local_dir(path: &Path) {
    // allows the tests to override the default values
    SUPER_AGENT_LOCAL_DATA_DIR_STATIC
        .get()
        .map(|m| *m.lock().unwrap() = path.to_string_lossy().to_string());
    // The Err variant in `set` just contains the value we attempted to set,
    // so we can just ignore the Result
    _ = SUPER_AGENT_LOCAL_DATA_DIR_STATIC
        .set(Mutex::new(path.to_string_lossy().to_string()))
        .inspect_err(|_| {
            debug!("attempted to initialize SUPER_AGENT_LOCAL_DATA_DIR but was already set")
        });
}

#[cfg(debug_assertions)]
pub fn set_remote_dir(path: &Path) {
    // allows the tests to override the default values
    SUPER_AGENT_DATA_DIR_STATIC
        .get()
        .map(|m| *m.lock().unwrap() = path.to_string_lossy().to_string());

    _ = SUPER_AGENT_DATA_DIR_STATIC
        .set(Mutex::new(path.to_string_lossy().to_string()))
        .inspect_err(|_| {
            debug!("attempted to initialize SUPER_AGENT_DATA_DIR but was already set")
        });
}

#[cfg(debug_assertions)]
pub(super) fn set_log_dir(path: &Path) {
    // allows the tests to override the default values
    SUPER_AGENT_LOG_DIR_STATIC
        .get()
        .map(|m| *m.lock().unwrap() = path.to_string_lossy().to_string());

    _ = SUPER_AGENT_LOG_DIR_STATIC
        .set(Mutex::new(path.to_string_lossy().to_string()))
        .inspect_err(|_| debug!("attempted to initialize SUPER_AGENT_LOG_DIR but was already set"));
}

#[cfg(debug_assertions)]
pub fn set_debug_mode_dirs(path: &Path) {
    debug!("setting data directories to the working directory");

    let local_path = path.join("nrsa_local");
    let remote_path = path.join("nrsa_remote");
    let log_path = path.join("nrsa_logs");

    set_local_dir(&local_path);
    set_remote_dir(&remote_path);
    set_log_dir(&log_path);
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
