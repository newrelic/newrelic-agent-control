use std::{fs::read_to_string, time::Duration};

use newrelic_agent_control::agent_control::{
    defaults::GENERATED_FOLDER_NAME,
    run::{BasePaths, Environment},
};
use tempfile::tempdir;

use crate::{
    common::{
        agent_control::start_agent_control_with_custom_config, opamp::FakeServer, retry::retry,
    },
    on_host::tools::{
        config::{create_agent_control_config, create_file, create_sub_agent_values},
        custom_agent_type::DYNAMIC_AGENT_TYPE_FILENAME,
    },
};

/// Given a agent-control with a sub-agent without supervised executables, it should be able to
/// read the health status from the file and send it to the opamp server.
#[test]
fn writes_filesystem_entries() {
    let opamp_server = FakeServer::start_new();

    let tempdir = tempdir().expect("failed to create temp dir");
    let local_dir = tempdir.path().join("local");
    let remote_dir = tempdir.path().join("remote");

    let expected_file_contents = "Hello, world!";
    let agent_id = "test-agent";
    let file_path = "randomdir/randomfile.txt";

    create_file(
        format!(
            r#"
namespace: test
name: test
version: 0.0.0
variables: {{}}
deployment:
  on_host:
    filesystem:
      somefile:
        path: {file_path}
        content: "{expected_file_contents}"
"#,
        ),
        local_dir.join(DYNAMIC_AGENT_TYPE_FILENAME),
    );

    let agents = format!(
        r#"
  {agent_id}:
    agent_type: "test/test:0.0.0"
"#
    );

    create_agent_control_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.to_path_buf(),
        opamp_server.cert_file_path(),
    );
    create_sub_agent_values(
        agent_id.to_string(),
        "".to_string(),
        local_dir.to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.to_path_buf(),
        remote_dir: remote_dir.to_path_buf(),
        log_dir: local_dir.to_path_buf(),
    };
    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), Environment::OnHost);

    let search_path = base_paths
        .remote_dir
        .join(GENERATED_FOLDER_NAME)
        .join(agent_id)
        .join(file_path);

    retry(30, Duration::from_secs(1), || {
        match read_to_string(&search_path) {
            Ok(s) if s == expected_file_contents => Ok(()),
            Ok(s) => Err(format!(
                "File content does not match. Expected {expected_file_contents} got: {s}"
            )
            .into()),
            Err(e) => {
                Err(format!("Failed to read file at {}: {}", search_path.display(), e).into())
            }
        }
    });
}
