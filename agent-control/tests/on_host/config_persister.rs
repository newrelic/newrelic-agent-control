use ::fs::directory_manager::{DirectoryManager, DirectoryManagerFs};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_type::definition::AgentTypeDefinition;
use newrelic_agent_control::agent_type::environment::Environment;
use newrelic_agent_control::agent_type::render::persister::config_persister::ConfigurationPersister;
use newrelic_agent_control::agent_type::render::persister::config_persister_file::ConfigurationPersisterFile;
use newrelic_agent_control::sub_agent::effective_agents_assembler::build_agent_type;
use newrelic_agent_control::values::yaml_config::YAMLConfig;
use std::fs;
use std::fs::Permissions;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

#[test]
// This test is the only one that writes to an actual file in the FS
fn test_configuration_persister_single_file() {
    let tempdir = tempfile::tempdir().unwrap();
    let mut temp_path = PathBuf::from(&tempdir.path());
    temp_path.push("test_configuration_persister_single_file");

    let dir_manager = DirectoryManagerFs;
    let res = dir_manager.create(temp_path.as_path(), Permissions::from_mode(0o700));

    assert!(res.is_ok());
    let persister = ConfigurationPersisterFile::new(temp_path.as_path());
    let agent_id = AgentID::new("some-agent-id").unwrap();

    let agent_type_definition: AgentTypeDefinition =
        serde_yaml::from_reader(AGENT_TYPE_SINGLE_FILE.as_bytes()).unwrap();
    let agent_type = build_agent_type(agent_type_definition, &Environment::OnHost).unwrap();
    let agent_values: YAMLConfig =
        serde_yaml::from_reader(AGENT_VALUES_SINGLE_FILE.as_bytes()).unwrap();
    let filled_variables = agent_type
        .variables
        .fill_with_values(agent_values)
        .unwrap()
        .flatten();

    assert!(persister
        .persist_agent_config(&agent_id.clone(), &filled_variables)
        .is_ok());

    temp_path.push("auto-generated");
    temp_path.push(agent_id);
    temp_path.push("newrelic-infra.yml");
    assert_eq!(
        EXPECTED_CONTENT_SINGLE_FILE,
        fs::read_to_string(temp_path.as_path()).unwrap()
    );
}

//////////////////////////////////////////////////
// Fixtures
//////////////////////////////////////////////////

const AGENT_TYPE_SINGLE_FILE: &str = r#"
namespace: newrelic
name: com.newrelic.infrastructure
version: 0.0.1
variables:
  on_host:
    config_file:
      description: "Newrelic infra configuration path"
      type: file
      required: true
      file_path: newrelic-infra.yml
deployment:
  on_host:
    executable:
      path: /usr/bin/newrelic-infra
      args: "--config=${config_file}"
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay_seconds: 5
"#;

const AGENT_VALUES_SINGLE_FILE: &str = r#"
config_file: |
  license_key: 1234567890987654321
  log:
    level: debug
"#;

const EXPECTED_CONTENT_SINGLE_FILE: &str = r#"license_key: 1234567890987654321
log:
  level: debug
"#;
