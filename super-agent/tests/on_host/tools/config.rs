use newrelic_super_agent::super_agent::defaults::{
    SUB_AGENT_DIR, SUPER_AGENT_CONFIG_FILE, VALUES_DIR, VALUES_FILE,
};
use std::fs::{create_dir_all, File};
use std::io::Write;
use std::path::PathBuf;

/// Creates the super-agent config given an opamp_server_endpoint
/// and a list of agents on the specified local_dir.
pub fn create_super_agent_config(
    opamp_server_endpoint: String,
    agents: String,
    local_dir: PathBuf,
) -> PathBuf {
    let config_file_path = local_dir.join(SUPER_AGENT_CONFIG_FILE);

    let mut local_file =
        File::create(config_file_path.clone()).expect("failed to create local config file");
    let super_agent_config = format!(
        r#"
host_id: integration-test
opamp:
  endpoint: {}
agents: {}
"#,
        opamp_server_endpoint, agents
    );
    write!(local_file, "{}", super_agent_config).unwrap();

    config_file_path
}

/// Creates a sub-agent values config for the agent_id provided on the base_dir
/// with the given content.
pub fn create_sub_agent_values(agent_id: String, config: String, base_dir: PathBuf) -> PathBuf {
    let agent_values_dir_path = base_dir.join(SUB_AGENT_DIR).join(agent_id).join(VALUES_DIR);
    create_dir_all(agent_values_dir_path.clone()).expect("failed to create values directory");

    let values_file_path = agent_values_dir_path.join(VALUES_FILE);
    let mut local_values_file =
        File::create(values_file_path.clone()).expect("failed to create local values file");
    write!(local_values_file, "{}", config).unwrap();

    values_file_path
}
