use std::fs::{File, create_dir_all};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_ID, FOLDER_NAME_FLEET_DATA, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
    STORE_KEY_OPAMP_DATA_CONFIG, default_capabilities,
};
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::on_host::file_store::{FileStore, build_config_name};
use newrelic_agent_control::values::ConfigRepo;
use newrelic_agent_control::values::config_repository::ConfigRepository;

/// Creates the agent-control config given an opamp_server_endpoint
/// and a list of agents on the specified local_dir.
pub fn create_agent_control_config(
    opamp_server_endpoint: String,
    jwks_endpoint: String,
    agents: String,
    local_dir: PathBuf,
) {
    create_agent_control_config_with_proxy(
        opamp_server_endpoint,
        jwks_endpoint,
        agents,
        local_dir,
        None,
    );
}

/// Extends [create_agent_control_config] with a proxy configuration parameter.
pub fn create_agent_control_config_with_proxy(
    opamp_server_endpoint: String,
    jwks_endpoint: String,
    agents: String,
    local_dir: PathBuf,
    proxy: Option<String>,
) {
    let proxy_config = proxy
        .map(|config| format!("proxy: {config}"))
        .unwrap_or_default();

    let agent_control_config = format!(
        r#"
host_id: integration-test
fleet_control:
  endpoint: {}
  poll_interval: 5s
  signature_validation:
    public_key_server_url: {}
agents: {}
{}
"#,
        opamp_server_endpoint, jwks_endpoint, agents, proxy_config,
    );
    create_file(
        agent_control_config,
        local_dir
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(AGENT_CONTROL_ID)
            .join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)),
    );
}

pub fn create_file(content: String, path: PathBuf) {
    create_dir_all(path.parent().unwrap()).unwrap();

    let mut local_file = File::create(path).expect("failed to create local config file");
    write!(local_file, "{content}").unwrap();
}

/// Creates local values config for the agent_id provided on the base_dir
/// with the given content.
pub fn create_local_config(agent_id: String, config: String, base_dir: PathBuf) {
    let agent_values_dir_path = base_dir.join(FOLDER_NAME_LOCAL_DATA).join(agent_id);
    create_dir_all(agent_values_dir_path.clone()).expect("failed to create values directory");

    let values_file_path =
        agent_values_dir_path.join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG));

    create_file(config, values_file_path.clone());
}

/// Creates remote values config for the agent_id provided on the base_dir
/// with the given content.
pub fn create_remote_config(agent_id: String, config: String, base_dir: PathBuf) {
    let agent_values_dir_path = base_dir.join(FOLDER_NAME_FLEET_DATA).join(agent_id);
    create_dir_all(agent_values_dir_path.clone()).expect("failed to create values directory");

    let values_file_path =
        agent_values_dir_path.join(build_config_name(STORE_KEY_OPAMP_DATA_CONFIG));

    create_file(config, values_file_path.clone());
}

pub fn load_remote_config_content(agent_id: &AgentID, base_paths: BasePaths) -> Option<String> {
    let file_store = Arc::new(FileStore::new_local_fs(
        base_paths.local_dir.clone(),
        base_paths.remote_dir.clone(),
    ));
    let yaml_config_repo = ConfigRepo::new(file_store).with_remote();

    yaml_config_repo
        .load_remote(agent_id, &default_capabilities())
        .unwrap()
        .map(|rc| serde_yaml::to_string(&rc.get_yaml_config()).unwrap())
}
