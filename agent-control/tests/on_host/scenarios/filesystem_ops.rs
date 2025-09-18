use std::{fs::read_to_string, path::Path, time::Duration};

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

/// An on-host agent definition that includes filesystem entries should result in the entries being
/// created in the appropriate location under the remote directory.
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
        opamp_server.jwks_endpoint(),
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
        read_file_and_expect_content(&search_path, expected_file_contents)?;
        Ok(())
    });
}

/// An on-host agent definition that includes filesystem entries should result in the entries being
/// created in the appropriate location under the remote directory and with their contents properly
/// rendered from the defined variables.
#[test]
fn complete_render_and_and_write_files() {
    let opamp_server = FakeServer::start_new();

    let tempdir = tempdir().expect("failed to create temp dir");
    let local_dir = tempdir.path().join("local");
    let remote_dir = tempdir.path().join("remote");

    let agent_id = "test-agent";
    // Rendered file paths
    let yaml_file_path = "randomdir/randomfile.yaml";
    let string_file_path = "randomdir-2/some_string.txt";
    // String variable and file contents
    let string_var_content = "Hello, world!";
    let yaml_var_content = "key: value";
    let expected_yaml_file_contents = format!("{yaml_var_content}\n"); // Writing YAML to file adds a trailing newline
    let expected_string_file_contents =
        format!("Some string contents with a rendered variable: {string_var_content}");

    create_file(
        format!(
            r#"
namespace: test
name: test
version: 0.0.0
variables:
  on_host:
    yaml_file_contents:
      description: "Contents of the YAML file"
      type: yaml
      required: true
    some_string:
      description: "Contents of an arbitrary string file"
      type: string
      required: true
deployment:
  on_host:
    filesystem:
      somefile:
        path: {yaml_file_path}
        content: |-
          ${{nr-var:yaml_file_contents}}
      otherfile:
        path: {string_file_path}
        content: "Some string contents with a rendered variable: ${{nr-var:some_string}}"
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
        opamp_server.jwks_endpoint(),
        agents.to_string(),
        local_dir.to_path_buf(),
        opamp_server.cert_file_path(),
    );
    create_sub_agent_values(
        agent_id.to_string(),
        format!(
            r#"
yaml_file_contents:
  {yaml_var_content}
some_string: "{string_var_content}""#
        ),
        local_dir.to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.to_path_buf(),
        remote_dir: remote_dir.to_path_buf(),
        log_dir: local_dir.to_path_buf(),
    };
    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), Environment::OnHost);

    let yaml_search_path = base_paths
        .remote_dir
        .join(GENERATED_FOLDER_NAME)
        .join(agent_id)
        .join(yaml_file_path);
    let string_search_path = base_paths
        .remote_dir
        .join(GENERATED_FOLDER_NAME)
        .join(agent_id)
        .join(string_file_path);

    retry(30, Duration::from_secs(1), || {
        read_file_and_expect_content(&yaml_search_path, &expected_yaml_file_contents)?;
        read_file_and_expect_content(&string_search_path, &expected_string_file_contents)?;
        Ok(())
    });
}

fn read_file_and_expect_content(
    path: impl AsRef<Path>,
    expected_content: impl AsRef<str>,
) -> Result<(), String> {
    let expected_content = expected_content.as_ref();
    match read_to_string(path.as_ref()) {
        Ok(s) if s == expected_content => Ok(()),
        Ok(s) => Err(format!(
            "File content does not match. Expected \"{expected_content}\" got: \"{s}\""
        )),
        Err(e) => Err(format!(
            "Failed to read file at {}: {}",
            path.as_ref().display(),
            e
        )),
    }
}
