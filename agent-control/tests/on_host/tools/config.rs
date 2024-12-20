use std::fs::{create_dir_all, File};
use std::io::Write;
use std::path::PathBuf;

use newrelic_agent_control::agent_control::config::AgentID;
use newrelic_agent_control::agent_control::defaults::{
    default_capabilities, AGENT_CONTROL_CONFIG_FILENAME, SUB_AGENT_DIR, VALUES_DIR, VALUES_FILENAME,
};
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::values::file::YAMLConfigRepositoryFile;
use newrelic_agent_control::values::yaml_config_repository::{
    YAMLConfigRepository, YAMLConfigRepositoryError,
};

/// Creates the agent-control config given an opamp_server_endpoint
/// and a list of agents on the specified local_dir.
pub fn create_agent_control_config(
    opamp_server_endpoint: String,
    agents: String,
    local_dir: PathBuf,
) {
    create_agent_control_config_with_proxy(opamp_server_endpoint, agents, local_dir, None);
}

/// Extends [create_agent_control_config] with a proxy configuration parameter.
pub fn create_agent_control_config_with_proxy(
    opamp_server_endpoint: String,
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
agents: {}
{}
"#,
        opamp_server_endpoint, agents, proxy_config,
    );
    create_file(
        agent_control_config,
        local_dir.join(AGENT_CONTROL_CONFIG_FILENAME),
    );
}

pub fn create_file(content: String, path: PathBuf) {
    let mut local_file = File::create(path).expect("failed to create local config file");
    write!(local_file, "{}", content).unwrap();
}

/// Creates a sub-agent values config for the agent_id provided on the base_dir
/// with the given content.
pub fn create_sub_agent_values(agent_id: String, config: String, base_dir: PathBuf) {
    let agent_values_dir_path = base_dir.join(SUB_AGENT_DIR).join(agent_id).join(VALUES_DIR);
    create_dir_all(agent_values_dir_path.clone()).expect("failed to create values directory");

    let values_file_path = agent_values_dir_path.join(VALUES_FILENAME);

    create_file(config, values_file_path.clone());
}

pub fn get_remote_config_content(
    agent_id: &AgentID,
    base_paths: BasePaths,
) -> Result<String, YAMLConfigRepositoryError> {
    let yaml_config_repo =
        YAMLConfigRepositoryFile::new(base_paths.local_dir.clone(), base_paths.remote_dir.clone())
            .with_remote();
    let remote_config = yaml_config_repo.load_remote(agent_id, &default_capabilities())?;
    match remote_config {
        None => Err(YAMLConfigRepositoryError::LoadError(
            "file not found...".to_string(),
        )),
        Some(remote_config) => Ok(serde_yaml::to_string(&remote_config).unwrap()),
    }
}
