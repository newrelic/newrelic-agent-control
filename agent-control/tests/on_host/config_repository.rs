use fs::directory_manager::{DirectoryManager, DirectoryManagerFs};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::{
    FOLDER_NAME_FLEET_DATA, STORE_KEY_OPAMP_DATA_CONFIG,
};
use newrelic_agent_control::opamp::instance_id::on_host::storer::build_config_name;
use newrelic_agent_control::opamp::remote_config::hash::{ConfigState, Hash};
use newrelic_agent_control::values::config::RemoteConfig;
use newrelic_agent_control::values::config_repository::ConfigRepository;
use newrelic_agent_control::values::file::ConfigRepositoryFile;
use std::fs::read_to_string;
use std::path::PathBuf;

// This test is the only one that writes to an actual file in the FS
#[test]
fn test_store_remote_no_mocks() {
    let tempdir = tempfile::tempdir().unwrap();

    let mut local_dir = PathBuf::from(&tempdir.path());
    local_dir.push("local_dir");

    let mut remote_dir = PathBuf::from(&tempdir.path());
    remote_dir.push("remote_dir");

    let dir_manager = DirectoryManagerFs;

    // Ensure dir exists
    let res = dir_manager.create(remote_dir.as_path());
    assert!(res.is_ok());

    let values_repo = ConfigRepositoryFile::new(local_dir.clone(), remote_dir.clone());

    let agent_id = AgentID::try_from("some-agent-id").unwrap();

    let agent_values = RemoteConfig {
        config: serde_yaml::from_reader(AGENT_VALUES_SINGLE_FILE.as_bytes()).unwrap(),
        hash: Hash::from("hash-test"),
        state: ConfigState::Applying,
    };

    values_repo
        .store_remote(&agent_id.clone(), &agent_values)
        .unwrap();

    assert_eq!(
        AGENT_VALUES_SINGLE_FILE_STORED,
        read_to_string(
            remote_dir
                .join(FOLDER_NAME_FLEET_DATA)
                .join(agent_id)
                .join(build_config_name(STORE_KEY_OPAMP_DATA_CONFIG))
        )
        .expect("Failed to read the config file")
    );
}

//////////////////////////////////////////////////
// Fixtures
//////////////////////////////////////////////////
const AGENT_VALUES_SINGLE_FILE: &str = r#"config_file: |
  license_key: 1234567890987654321
  log:
    level: debug
"#;

const AGENT_VALUES_SINGLE_FILE_STORED: &str = r#"config:
  config_file: |
    license_key: 1234567890987654321
    log:
      level: debug
hash: hash-test
state: applying
"#;
