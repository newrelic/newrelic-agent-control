use ::fs::directory_manager::{DirectoryManager, DirectoryManagerFs};
use newrelic_super_agent::agent_type::agent_values::AgentValues;
use newrelic_super_agent::super_agent::config::AgentID;
use newrelic_super_agent::values::on_host::ValuesRepositoryFile;
use newrelic_super_agent::values::values_repository::ValuesRepository;
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

    let dir_manager = DirectoryManagerFs::default();

    // Ensure dir exists
    let res = dir_manager.create(remote_dir.as_path(), Permissions::from_mode(0o700));
    assert!(res.is_ok());

    let values_repo = ValuesRepositoryFile::default()
        .with_remote_conf_path(remote_dir.to_str().unwrap().to_string());

    let agent_id = AgentID::new("some-agent-id").unwrap();

    let agent_values: AgentValues =
        serde_yaml::from_reader(AGENT_VALUES_SINGLE_FILE.as_bytes()).unwrap();

    values_repo
        .store_remote(&agent_id.clone(), &agent_values)
        .unwrap();

    remote_dir.push(agent_id);
    remote_dir.push("values");
    remote_dir.push("values.yaml");

    assert_eq!(
        AGENT_VALUES_SINGLE_FILE,
        fs::read_to_string(remote_dir.as_path()).unwrap()
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
