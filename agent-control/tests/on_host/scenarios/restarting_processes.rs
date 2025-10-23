#![cfg(target_family = "unix")]
use assert_cmd::{Command, cargo::cargo_bin_cmd};
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_ID, DYNAMIC_AGENT_TYPE_DIR, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
};
use newrelic_agent_control::opamp::instance_id::on_host::storer::build_config_name;
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

/// The agent control is configured with no OpAMP and a custom agent-type to check that
/// a process gets restarted as expected when killing it externally.
#[test]
#[ignore = "requires root"]
fn killing_subprocess_with_signal_restarts_as_root() -> Result<(), Box<dyn std::error::Error>> {
    use crate::on_host::cli::create_temp_file;

    let dir = TempDir::new()?;
    let _agent_type_def = create_temp_file(
        dir.path().join(DYNAMIC_AGENT_TYPE_DIR).as_path(),
        "test-agent.yaml",
        r#"
namespace: newrelic
name: com.newrelic.test-agent
version: 0.0.1
variables:
  on_host:
    duration-1:
      description: "time to sleep"
      type: string
      required: false
      default: "yes"
    duration-2:
      description: "time to sleep"
      type: string
      required: false
      default: "yes"
deployment:
  on_host:
    enable_file_logging: false
    executables:
      - id: sleep1
        path: sleep
        args: "${nr-var:duration-1}"
        restart_policy:
          backoff_strategy:
            type: fixed
            max_retries: 2
            backoff_delay: 0s
      - id: sleep2
        path: sleep
        args: "${nr-var:duration-2}"
        restart_policy:
          backoff_strategy:
            type: fixed
            max_retries: 2
            backoff_delay: 0s
"#,
    );

    let _values_file = create_temp_file(
        dir.path()
            .join(FOLDER_NAME_LOCAL_DATA)
            .join("test-agent")
            .as_path(),
        "local_config.yaml",
        r#"
duration-1: "1000000"
duration-2: "2000000"
"#,
    );

    let _config_path = create_temp_file(
        &dir.path()
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(AGENT_CONTROL_ID),
        build_config_name(STORE_KEY_LOCAL_DATA_CONFIG).as_str(),
        r#"
log:
  level: debug
  file:
    enabled: true
agents:
  test-agent:
    agent_type: newrelic/com.newrelic.test-agent:0.0.1
"#,
    );

    // we need to avoid dropping the variable, therefore we pass only the path
    let path = dir.path().to_path_buf();

    let agent_control_join = thread::spawn(move || {
        let mut cmd = cargo_bin_cmd!("newrelic-agent-control");
        cmd.arg("--local-dir").arg(path);
        // cmd_assert is not made for long running programs, so we kill it anyway after 10 seconds
        cmd.timeout(Duration::from_secs(10));
        // But in any case we make sure that it actually attempted to create the supervisor group,
        // so it works when the program is run as root
        let logs = cmd.output().expect("failed to execute process").stdout;
        println!("{}", String::from_utf8(logs).unwrap());
    });

    thread::sleep(Duration::from_secs(5));

    // Use `pgrep` to find the process id of both sleep commands
    // It is expected that only one such process is found!
    let sleep_pid = Command::new("pgrep")
        .arg("-f")
        .arg("sleep 1000000")
        .output()
        .expect("failed to execute process")
        .stdout;

    let sleep_pid = String::from_utf8(sleep_pid).unwrap();

    let second_sleep_pid = Command::new("pgrep")
        .arg("-f")
        .arg("sleep 2000000")
        .output()
        .expect("failed to execute process")
        .stdout;

    thread::sleep(Duration::from_secs(35));
    let second_sleep_pid = String::from_utf8(second_sleep_pid).unwrap();

    // Send a SIGKILL to both yes commands
    signal::kill(
        Pid::from_raw(sleep_pid.trim().parse::<i32>().unwrap()),
        Signal::SIGKILL,
    )
    .unwrap();
    signal::kill(
        Pid::from_raw(second_sleep_pid.trim().parse::<i32>().unwrap()),
        Signal::SIGKILL,
    )
    .unwrap();

    // Wait for the agent-control to restart the process
    thread::sleep(Duration::from_secs(2));

    // Get the pid for both new yes command
    let new_sleep_pid = Command::new("pgrep")
        .arg("-f")
        .arg("sleep 1000000")
        .output()
        .expect("failed to execute process")
        .stdout;

    let new_sleep_pid = String::from_utf8(new_sleep_pid).unwrap();

    let new_second_sleep_pid = Command::new("pgrep")
        .arg("-f")
        .arg("sleep 2000000")
        .output()
        .expect("failed to execute process")
        .stdout;

    let new_second_sleep_pid = String::from_utf8(new_second_sleep_pid).unwrap();

    agent_control_join.join().unwrap();

    // Assert the PIDs are different
    assert_ne!(sleep_pid.trim(), new_sleep_pid.trim());
    assert_ne!(second_sleep_pid.trim(), new_second_sleep_pid.trim());

    Ok(())
}
