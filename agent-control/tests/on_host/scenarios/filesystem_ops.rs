use std::{fs::read_to_string, path::Path, time::Duration};

use crate::on_host::consts::NO_CONFIG;
use crate::{
    common::{
        agent_control::start_agent_control_with_custom_config, retry::retry, runtime::tokio_runtime,
    },
    on_host::tools::{
        config::{create_agent_control_config, create_file, create_local_config},
        custom_agent_type::DYNAMIC_AGENT_TYPE_FILENAME,
    },
};
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use newrelic_agent_control::agent_control::{
    defaults::AGENT_FILESYSTEM_FOLDER_NAME, run::BasePaths,
};
use tempfile::tempdir;

/// An on-host agent definition that includes filesystem entries should result in the entries being
/// created in the appropriate location under the remote directory.
#[test]
fn writes_filesystem_entries() {
    let opamp_server = FakeServer::start(tokio_runtime().handle());

    let tempdir = tempdir().expect("failed to create temp dir");
    let local_dir = tempdir.path().join("local");
    let remote_dir = tempdir.path().join("remote");

    let expected_file_contents = "Hello, world!";
    let agent_id = "test-agent";
    let dir_entry = "example-filepath";
    let file_path = "randomdir/randomfile.txt";

    create_file(
        format!(
            r#"
namespace: test
name: test
version: 0.0.0
variables: {{}}
deployment:
  linux:
    filesystem:
      {dir_entry}:
        {file_path}: "{expected_file_contents}"
  windows:
    filesystem:
      {dir_entry}:
        {file_path}: "{expected_file_contents}"
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
    );
    create_local_config(
        agent_id.to_string(),
        NO_CONFIG.to_string(),
        local_dir.to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.to_path_buf(),
        remote_dir: remote_dir.to_path_buf(),
        log_dir: local_dir.to_path_buf(),
    };
    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

    let search_path = base_paths
        .remote_dir
        .join(AGENT_FILESYSTEM_FOLDER_NAME)
        .join(agent_id)
        .join(dir_entry)
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
fn complete_render_and_and_write_files_and_dirs() {
    let opamp_server = FakeServer::start(tokio_runtime().handle());

    let tempdir = tempdir().expect("failed to create temp dir");
    let local_dir = tempdir.path().join("local");
    let remote_dir = tempdir.path().join("remote");

    let agent_id = "test-agent";

    // Rendered file paths
    let yaml_file_path = "randomdir/randomfile.yaml";
    let string_file_path = "randomdir-2/some_string.txt";

    // Rendered directory paths
    let dir_path = "somedir";
    let fully_templated_dir = "fully_templated_dir";

    // String variable and file contents
    let string_var_content = "Hello, world!";
    let yaml_var_content = "key: value";
    let expected_yaml_file_contents = format!("{yaml_var_content}\n"); // Writing YAML to file adds a trailing newline
    let expected_string_file_contents =
        format!("Some string contents with a rendered variable: {string_var_content}");

    // Directory files and their contents. First element is the file name, second is the expected contents
    let expected_dir_file1 = ("file1.txt", "File 1 contents".to_string());
    let expected_dir_file2 = (
        "file2.txt",
        format!("File 2 contents with a variable: {string_var_content}\n"),
    );
    let expected_dir_file3 = ("file3.txt", "File 3 contents".to_string());
    let expected_dir_file4 = (
        "file4.txt",
        format!("File 4 contents with a variable: {string_var_content}\n"),
    );
    let expected_dir_file5 = ("file5.yaml", "my_key: my_value\nmy_seq:\n- item1\n- item2\nmy_string: |-\n  This is a multi-line\n  string in YAML\n".to_string());

    // Create agent type definition
    create_file(
        format!(
            r#"
namespace: test
name: test
version: 0.0.0
variables:
  common:
    yaml_file_contents:
      description: "Contents of the YAML file"
      type: yaml
      required: true
    some_string:
      description: "Contents of an arbitrary string file"
      type: string
      required: true
    some_mapstringyaml:
      description: "A directory structure"
      type: map[string]yaml
      required: true
deployment:
  windows:
    filesystem:
      randomdir:
        "{yaml_file_path}": |-
          ${{nr-var:yaml_file_contents}}
        "{string_file_path}": "Some string contents with a rendered variable: ${{nr-var:some_string}}"
      {dir_path}:
        file1.txt: "File 1 contents"
        file2.txt: |
          File 2 contents with a variable: ${{nr-var:some_string}}
      "{fully_templated_dir}": ${{nr-var:some_mapstringyaml}}
  linux:
    filesystem:
      randomdir:
        "{yaml_file_path}": |-
          ${{nr-var:yaml_file_contents}}
        "{string_file_path}": "Some string contents with a rendered variable: ${{nr-var:some_string}}"
      {dir_path}:
        file1.txt: "File 1 contents"
        file2.txt: |
          File 2 contents with a variable: ${{nr-var:some_string}}
      "{fully_templated_dir}": ${{nr-var:some_mapstringyaml}}
"#,
        ),
        local_dir.join(DYNAMIC_AGENT_TYPE_FILENAME),
    );

    // Create AC config
    create_agent_control_config(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        format!(
            r#"
  {agent_id}:
    agent_type: "test/test:0.0.0"
"#
        ),
        local_dir.to_path_buf(),
    );
    // Values. Contains 3 variables: a YAML, a string, and a map[string]yaml (to create files in a directory)
    create_local_config(
        agent_id.to_string(),
        format!(
            r#"
yaml_file_contents:
  {yaml_var_content}
some_string: "{string_var_content}"
some_mapstringyaml:
  file3.txt: "File 3 contents"
  file4.txt: |
    File 4 contents with a variable: {string_var_content}
  file5.yaml:
    my_key: my_value
    my_seq:
        - item1
        - item2
    my_string: |-
        This is a multi-line
        string in YAML
"#
        ),
        local_dir.to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.to_path_buf(),
        remote_dir: remote_dir.to_path_buf(),
        log_dir: local_dir.to_path_buf(),
    };
    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

    // Rendered files
    let yaml_search_path = base_paths
        .remote_dir
        .join(AGENT_FILESYSTEM_FOLDER_NAME)
        .join(agent_id)
        .join("randomdir")
        .join(yaml_file_path);
    let string_search_path = base_paths
        .remote_dir
        .join(AGENT_FILESYSTEM_FOLDER_NAME)
        .join(agent_id)
        .join("randomdir")
        .join(string_file_path);
    let dir_search_path = base_paths
        .remote_dir
        .join(AGENT_FILESYSTEM_FOLDER_NAME)
        .join(agent_id)
        .join(dir_path);
    let fully_templated_dir_search_path = base_paths
        .remote_dir
        .join(AGENT_FILESYSTEM_FOLDER_NAME)
        .join(agent_id)
        .join(fully_templated_dir);

    let expected_files_with_contents = [
        (yaml_search_path, expected_yaml_file_contents),
        (string_search_path, expected_string_file_contents),
        (
            dir_search_path.join(expected_dir_file1.0),
            expected_dir_file1.1,
        ),
        (
            dir_search_path.join(expected_dir_file2.0),
            expected_dir_file2.1,
        ),
        (
            fully_templated_dir_search_path.join(expected_dir_file3.0),
            expected_dir_file3.1,
        ),
        (
            fully_templated_dir_search_path.join(expected_dir_file4.0),
            expected_dir_file4.1,
        ),
        (
            fully_templated_dir_search_path.join(expected_dir_file5.0),
            expected_dir_file5.1,
        ),
    ];

    retry(30, Duration::from_secs(1), || {
        for (path, contents) in expected_files_with_contents.iter() {
            read_file_and_expect_content(path, contents)?;
        }
        Ok(())
    });
}

/// Filesystem entries should persist across agent control restarts.
/// This test verifies that directories like newrelic-infra/newrelic-integrations/logging
/// are not deleted when agent control restarts, ensuring persistent storage across restarts.
#[test]
fn filesystem_persists_across_restarts() {
    let opamp_server = FakeServer::start(tokio_runtime().handle());

    let tempdir = tempdir().expect("failed to create temp dir");
    let local_dir = tempdir.path().join("local");
    let remote_dir = tempdir.path().join("remote");

    let agent_id = "test-agent";
    let config_content = "license_key: test_key\nlog_level: info\n";
    let integrations_content = "integration: test\n";
    let logging_content = "fluent_bit: true\n";

    // Create agent type definition with filesystem structure similar to newrelic-infra
    create_file(
        r#"
namespace: test
name: infra-agent
version: 0.0.0
variables:
  common:
    config_agent:
      description: "Agent configuration"
      type: yaml
      required: true
    config_integrations:
      description: "Integrations configuration"
      type: yaml
      required: true
    config_logging:
      description: "Logging configuration"
      type: yaml
      required: true
deployment:
  linux:
    filesystem:
      config:
        newrelic-infra.yaml: |-
          ${nr-var:config_agent}
      integrations.d:
        integration.yaml: |-
          ${nr-var:config_integrations}
      logging.d:
        logging.yaml: |-
          ${nr-var:config_logging}
      # This directory needs to persist across restarts for the infra agent
      newrelic-infra/newrelic-integrations/logging: {}
  windows:
    filesystem:
      config:
        newrelic-infra.yaml: |-
          ${nr-var:config_agent}
      integrations.d:
        integration.yaml: |-
          ${nr-var:config_integrations}
      logging.d:
        logging.yaml: |-
          ${nr-var:config_logging}
      # This directory needs to persist across restarts for the infra agent
      newrelic-infra/newrelic-integrations/logging: {}
"#
        .to_string(),
        local_dir.join(DYNAMIC_AGENT_TYPE_FILENAME),
    );

    let agents = format!(
        r#"
  {agent_id}:
    agent_type: "test/infra-agent:0.0.0"
"#
    );

    create_agent_control_config(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        agents.to_string(),
        local_dir.to_path_buf(),
    );

    create_local_config(
        agent_id.to_string(),
        r#"
config_agent:
  license_key: test_key
  log_level: info
config_integrations:
  integration: test
config_logging:
  fluent_bit: true
"#
        .to_string(),
        local_dir.to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.to_path_buf(),
        remote_dir: remote_dir.to_path_buf(),
        log_dir: local_dir.to_path_buf(),
    };

    // Define expected file paths (used throughout the test)
    let config_file_path = base_paths
        .remote_dir
        .join(AGENT_FILESYSTEM_FOLDER_NAME)
        .join(agent_id)
        .join("config")
        .join("newrelic-infra.yaml");

    let integrations_file_path = base_paths
        .remote_dir
        .join(AGENT_FILESYSTEM_FOLDER_NAME)
        .join(agent_id)
        .join("integrations.d")
        .join("integration.yaml");

    let logging_file_path = base_paths
        .remote_dir
        .join(AGENT_FILESYSTEM_FOLDER_NAME)
        .join(agent_id)
        .join("logging.d")
        .join("logging.yaml");

    let persistent_dir_path = base_paths
        .remote_dir
        .join(AGENT_FILESYSTEM_FOLDER_NAME)
        .join(agent_id)
        .join("newrelic-infra")
        .join("newrelic-integrations")
        .join("logging");

    let test_file_path = persistent_dir_path.join("test.yaml");

    // First agent control run - creates the filesystem structure
    {
        let _agent_control =
            start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

        // Wait for files to be created
        retry(30, Duration::from_secs(1), || {
            read_file_and_expect_content(&config_file_path, config_content)?;
            read_file_and_expect_content(&integrations_file_path, integrations_content)?;
            read_file_and_expect_content(&logging_file_path, logging_content)?;

            // Verify the persistent directory exists
            if !persistent_dir_path.exists() {
                return Err(format!(
                    "Persistent directory does not exist: {}",
                    persistent_dir_path.display()
                )
                .into());
            }

            Ok(())
        });
        // Agent control is dropped here, simulating a shutdown
    }

    // Verify files still exist on disk after shutdown, before restart
    read_file_and_expect_content(&config_file_path, config_content)
        .expect("Config file content should match after shutdown");
    read_file_and_expect_content(&integrations_file_path, integrations_content)
        .expect("Integrations file content should match after shutdown");
    read_file_and_expect_content(&logging_file_path, logging_content)
        .expect("Logging file content should match after shutdown");

    assert!(
        persistent_dir_path.exists(),
        "Persistent directory should still exist after shutdown: {}",
        persistent_dir_path.display()
    );

    // Simulate a file created by the infra agent in the persistent directory
    create_file("test\n".to_string(), test_file_path.clone());

    // Second agent control run - simulates restart
    {
        let _agent_control =
            start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

        // Verify all files and directories still exist after restart
        retry(30, Duration::from_secs(1), || {
            read_file_and_expect_content(&config_file_path, config_content)?;
            read_file_and_expect_content(&integrations_file_path, integrations_content)?;
            read_file_and_expect_content(&logging_file_path, logging_content)?;

            if !persistent_dir_path.exists() {
                return Err(format!(
                    "Persistent directory was deleted after restart: {}",
                    persistent_dir_path.display()
                )
                .into());
            }

            // Verify the test file created in the persistent directory still exists
            read_file_and_expect_content(&test_file_path, "test\n")?;

            Ok(())
        });
    }
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
