#![cfg(unix)]
use assert_cmd::Command;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_CONFIG_FILENAME, DYNAMIC_AGENT_TYPE_FILENAME,
};
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
        &dir,
        DYNAMIC_AGENT_TYPE_FILENAME,
        r#"
namespace: newrelic
name: com.newrelic.test-agent
version: 0.0.1
variables:
  on_host:
    duration:
      description: "time to sleep"
      type: string
      required: false
      default: "yes"
deployment:
  on_host:
    enable_file_logging: false
    executable:
      path: sleep
      args: "${nr-var:duration}"
      restart_policy:
        backoff_strategy:
          type: fixed
          max_retries: 2
          backoff_delay: 0s
"#,
    );

    let _values_file = create_temp_file(
        &dir,
        "fleet/agents.d/test-agent/values/values.yaml",
        r#"
duration: "1000000"
"#,
    );

    let _config_path = create_temp_file(
        &dir,
        AGENT_CONTROL_CONFIG_FILENAME,
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
        let mut cmd = Command::cargo_bin("newrelic-agent-control-onhost").unwrap();
        cmd.arg("--local-dir").arg(path);
        // cmd_assert is not made for long running programs, so we kill it anyway after 10 seconds
        cmd.timeout(Duration::from_secs(10));
        // But in any case we make sure that it actually attempted to create the supervisor group,
        // so it works when the program is run as root
        let logs = cmd.output().expect("failed to execute process").stdout;
        println!("{}", String::from_utf8(logs).unwrap());
    });

    thread::sleep(Duration::from_secs(5));

    // Use `pgrep` to find the process id of the yes command
    // It is expected that only one such process is found!
    let yes_pid = Command::new("pgrep")
        .arg("-f")
        .arg("sleep 1000000")
        .output()
        .expect("failed to execute process")
        .stdout;

    let yes_pid = String::from_utf8(yes_pid).unwrap();

    println!("PID {}", yes_pid);

    // Send a SIGKILL to the yes command
    signal::kill(
        Pid::from_raw(yes_pid.trim().parse::<i32>().unwrap()),
        Signal::SIGKILL,
    )
    .unwrap();

    // Wait for the agent-control to restart the process
    thread::sleep(Duration::from_secs(2));

    // Get the pid for the new yes command
    let new_yes_pid = Command::new("pgrep")
        .arg("-f")
        .arg("sleep 1000000")
        .output()
        .expect("failed to execute process")
        .stdout;

    let new_yes_pid = String::from_utf8(new_yes_pid).unwrap();

    agent_control_join.join().unwrap();

    // Assert the PID is different
    assert_ne!(yes_pid, new_yes_pid);

    Ok(())
}
