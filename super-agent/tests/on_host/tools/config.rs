use std::fs::{create_dir_all, File};
use std::io::Write;
use std::path::PathBuf;

use newrelic_super_agent::super_agent::config::AgentID;
use newrelic_super_agent::super_agent::defaults::{
    default_capabilities, SUB_AGENT_DIR, SUPER_AGENT_CONFIG_FILE, VALUES_DIR, VALUES_FILE,
};
use newrelic_super_agent::super_agent::run::BasePaths;
use newrelic_super_agent::values::file::YAMLConfigRepositoryFile;
use newrelic_super_agent::values::yaml_config_repository::{
    YAMLConfigRepository, YAMLConfigRepositoryError,
};

/// Creates the super-agent config given an opamp_server_endpoint
/// and a list of agents on the specified local_dir.
pub fn create_super_agent_config(
    opamp_server_endpoint: String,
    agents: String,
    local_dir: PathBuf,
) {
    let super_agent_config = format!(
        r#"
host_id: integration-test
opamp:
  endpoint: {}
agents: {}
"#,
        opamp_server_endpoint, agents
    );

    create_file(super_agent_config, local_dir.join(SUPER_AGENT_CONFIG_FILE));
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

    let values_file_path = agent_values_dir_path.join(VALUES_FILE);

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
