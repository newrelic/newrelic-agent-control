use ::fs::directory_manager::{DirectoryManager, DirectoryManagerFs};
use newrelic_agent_control::agent_control::config::AgentID;
use newrelic_agent_control::opamp::remote_config::hash::Hash;
use newrelic_agent_control::opamp::remote_config::status::AgentRemoteConfigStatus;
use newrelic_agent_control::opamp::remote_config::status_manager::local_filesystem::{
    concatenate_sub_agent_status_dir_path, FileSystemConfigStatusManager,
};
use newrelic_agent_control::opamp::remote_config::status_manager::ConfigStatusManager;
use newrelic_agent_control::values::yaml_config::YAMLConfig;
use std::fs;
use std::fs::Permissions;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
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
    let res = dir_manager.create(remote_dir.as_path(), Permissions::from_mode(0o700));
    assert!(res.is_ok());

    let values_repo =
        FileSystemConfigStatusManager::new(local_dir.clone()).with_remote(remote_dir.clone());

    let agent_id = AgentID::new("some-agent-id").unwrap();

    let hash = Hash::new("some-hash".to_string());
    let agent_values: YAMLConfig =
        serde_yaml::from_reader(AGENT_VALUES_SINGLE_FILE.as_bytes()).unwrap();
    let remote_status = AgentRemoteConfigStatus {
        status_hash: hash,
        remote_config: Some(agent_values),
    };

    values_repo
        .store_remote_status(&agent_id.clone(), &remote_status)
        .unwrap();

    let expected_path = concatenate_sub_agent_status_dir_path(&remote_dir, &agent_id);
    assert_eq!(
        AGENT_STATUS_SINGLE_FILE,
        fs::read_to_string(expected_path).unwrap()
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

const AGENT_STATUS_SINGLE_FILE: &str = r#"status_hash:
  hash: some-hash
  state: applying
remote_config:
  config_file: |
    license_key: 1234567890987654321
    log:
      level: debug
"#;
