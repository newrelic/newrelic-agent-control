use ::fs::directory_manager::{DirectoryManager, DirectoryManagerFs};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::opamp::remote_config::hash::Hash;
use newrelic_agent_control::values::config::RemoteConfig;
use newrelic_agent_control::values::config_repository::ConfigRepository;
use newrelic_agent_control::values::file::{ConfigRepositoryFile, concatenate_sub_agent_dir_path};
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

    let values_repo = ConfigRepositoryFile::new(local_dir.clone(), remote_dir.clone());

    let agent_id = AgentID::new("some-agent-id").unwrap();

    let agent_values = RemoteConfig::new(
        serde_yaml::from_reader(AGENT_VALUES_SINGLE_FILE.as_bytes()).unwrap(),
        Hash::new("hash-test".to_string()),
    );

    values_repo
        .store_remote(&agent_id.clone(), &agent_values)
        .unwrap();

    assert_eq!(
        AGENT_VALUES_SINGLE_FILE_STORED,
        fs::read_to_string(concatenate_sub_agent_dir_path(&remote_dir, &agent_id)).unwrap()
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
