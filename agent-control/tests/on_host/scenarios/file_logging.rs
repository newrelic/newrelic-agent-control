use std::{path::Path, time::Duration};

use crate::{
    common::{
        agent_control::start_agent_control_with_custom_config,
        health::check_latest_health_status_was_healthy, opamp::FakeServer, retry::retry,
    },
    on_host::tools::{
        config::{create_agent_control_config, create_file, create_local_config},
        custom_agent_type::DYNAMIC_AGENT_TYPE_FILENAME,
        instance_id::get_instance_id,
    },
};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use rstest::rstest;
use tempfile::tempdir;

const LOGGING_AGENT_TYPE_YAML: &str = r#"
namespace: test
name: file-logging-agent
version: 0.0.0
variables:
  common:
    message:
      description: "Message to echo to stdout"
      type: "string"
      required: true
    enable_file_logging:
      description: "Whether to enable file logging"
      type: "string"
      required: true
deployment:
  linux:
    enable_file_logging: ${nr-var:enable_file_logging}
    executables:
      - id: echo-agent
        path: sh
        args:
          - tests/on_host/data/echo_and_sleep.sh
          - "${nr-var:message}"
  windows:
    enable_file_logging: ${nr-var:enable_file_logging}
    executables:
      - id: echo-agent
        path: powershell.exe
        args:
          - -NoProfile
          - -ExecutionPolicy
          - Bypass
          - -File
          - tests\\on_host\\data\\echo_and_sleep.ps1
          - -Message
          - "${nr-var:message}"
"#;

/// Collects all stdout log file contents for the given agent under the log directory.
/// Returns the merged contents of all stdout log files, or an empty string if the directory
/// does not exist.
fn collect_stdout_logs(log_dir: &std::path::Path, agent_id: &str) -> String {
    let agent_logs_dir = log_dir.join(agent_id);
    if !agent_logs_dir.exists() {
        return String::new();
    }

    std::fs::read_dir(agent_logs_dir)
        .expect("should read logs dir")
        .map(|entry| entry.expect("entry").path())
        .filter(|p| {
            p.file_prefix()
                .is_some_and(|n| n.to_string_lossy().starts_with("stdout"))
        })
        .map(|p| std::fs::read_to_string(p).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Starts agent control with the file-logging agent type, provides an initial config via local
/// values, waits for healthy, then sends a remote config update via OpAMP and waits for the
/// reload to take effect.
///
/// Returns the log_dir so callers can inspect the filesystem.
///
/// # Arguments
/// * `agent_id` - the agent id to use for the sub-agent
/// * `initial_file_logging` - initial value of `enable_file_logging` variable
/// * `initial_message` - message echoed to stdout in the first run
/// * `reload_file_logging` - value of `enable_file_logging` after reload
/// * `reload_message` - message echoed to stdout after reload
fn run_file_logging_scenario(
    agent_id: &str,
    initial_file_logging: bool,
    initial_message: &str,
    reload_file_logging: bool,
    reload_message: &str,
) -> (tempfile::TempDir, String) {
    let mut opamp_server = FakeServer::start_new();

    let tempdir = tempdir().expect("failed to create temp dir");
    let local_dir = tempdir.path().join("local");
    let remote_dir = tempdir.path().join("remote");
    let log_dir = tempdir.path().join("logs");

    // Write the agent type definition
    create_file(
        LOGGING_AGENT_TYPE_YAML.to_string(),
        local_dir.join(DYNAMIC_AGENT_TYPE_FILENAME),
    );

    // Configure agent control with the agent
    let agents = format!(
        r#"
  {agent_id}:
    agent_type: "test/file-logging-agent:0.0.0"
"#
    );
    create_agent_control_config(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        agents,
        local_dir.to_path_buf(),
    );

    // Provide the initial local config for the sub-agent
    create_local_config(
        agent_id.to_string(),
        format!(
            "message: \"{initial_message}\"\nenable_file_logging: \"{initial_file_logging}\"\n"
        ),
        local_dir.to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.to_path_buf(),
        remote_dir: remote_dir.to_path_buf(),
        log_dir: log_dir.to_path_buf(),
    };

    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

    let sub_agent_instance_id = get_instance_id(&AgentID::try_from(agent_id).unwrap(), base_paths);

    // Wait for the sub-agent to become healthy (meaning the first run started)
    retry(60, Duration::from_secs(1), || {
        check_latest_health_status_was_healthy(&opamp_server, &sub_agent_instance_id)
    });

    // Give some time for the echo output to be captured in the log files
    std::thread::sleep(Duration::from_secs(3));

    // Trigger a reload by sending a new remote config via OpAMP with updated values
    opamp_server.set_config_response(
        sub_agent_instance_id.clone(),
        format!("message: \"{reload_message}\"\nenable_file_logging: \"{reload_file_logging}\"\n"),
    );

    // Wait for the sub-agent to become healthy again after reload
    retry(60, Duration::from_secs(1), || {
        check_latest_health_status_was_healthy(&opamp_server, &sub_agent_instance_id)
    });

    // Give some time for the echo output to be captured in the log files
    std::thread::sleep(Duration::from_secs(3));

    // Return tempdir (to keep it alive) and the log_dir path as string
    // We return agent_id so callers know where to look for logs
    (tempdir, log_dir.to_string_lossy().to_string())
}

/// File logging enable/disable combinations with before and after reload checks
#[rstest]
#[case::onhost_supervisor_reloading_keeps_file_logging(true, "logs_run1", true, "logs_run2")]
#[case::onhost_supervisor_reloading_enables_file_logging(false, "logs_run1", true, "logs_run2")]
#[case::onhost_supervisor_reloading_disables_file_logging(true, "logs_run1", false, "logs_run2")]
fn test_file_logging_reload(
    #[case] first_run_enabled: bool,
    #[case] first_run_message: &str,
    #[case] second_run_enabled: bool,
    #[case] second_run_message: &str,
) {
    let agent_id = format!("file-logging-agent-{first_run_enabled}-{second_run_enabled}");

    let (_tempdir, log_dir) = run_file_logging_scenario(
        &agent_id,
        first_run_enabled,
        first_run_message,
        second_run_enabled,
        second_run_message,
    );

    let log_dir_path = Path::new(&log_dir);
    let agent_logs_dir = log_dir_path.join(&agent_id);
    assert!(
        agent_logs_dir.exists(),
        "Log directory {agent_logs_dir:?} does not exist"
    );

    let all_contents = collect_stdout_logs(log_dir_path, &agent_id);

    // If the logs are enabled for the run the string must be found, same for disabled and not found
    assert_eq!(
        all_contents.contains(first_run_message),
        first_run_enabled,
        "First run log not found (pre-reload). Log contents: {all_contents}"
    );
    assert_eq!(
        all_contents.contains(second_run_message),
        second_run_enabled,
        "Second run log not found (post-reload). Log contents: {all_contents}"
    );
}

/// File logging is disabled initially and stays disabled after reload.
/// No log directory should be created for the agent.
#[test]
fn onhost_supervisor_reloading_keeps_file_logging_disabled() {
    let unique_str_1 = "keeps_disabled_run1";
    let unique_str_2 = "keeps_disabled_run2";
    let agent_id = "test-agent-logs-always-disabled";

    let (_tempdir, log_dir) =
        run_file_logging_scenario(agent_id, false, unique_str_1, false, unique_str_2);

    let log_dir_path = Path::new(&log_dir);
    let agent_logs_dir = log_dir_path.join(agent_id);
    assert!(
        !agent_logs_dir.exists(),
        "Log directory {:?} should NOT exist (logging was never enabled)",
        agent_logs_dir
    );
}
