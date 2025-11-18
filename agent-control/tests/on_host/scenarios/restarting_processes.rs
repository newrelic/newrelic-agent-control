use crate::common::process_finder::find_processes_by_pattern;
use crate::on_host::cli::create_temp_file;
use assert_cmd::cargo::cargo_bin_cmd;
use newrelic_agent_control::{
    agent_control::defaults::{
        AGENT_CONTROL_ID, DYNAMIC_AGENT_TYPE_DIR, FOLDER_NAME_LOCAL_DATA,
        STORE_KEY_LOCAL_DATA_CONFIG,
    },
    on_host::file_store::build_config_name,
};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[cfg(target_family = "unix")]
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};

/// The agent control is configured with no OpAMP and a custom agent-type to check that
/// a process gets restarted as expected when killing it externally.
#[test]
#[ignore = "requires root"]
fn killing_subprocess_with_signal_restarts_as_root() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;

    #[cfg(target_family = "unix")]
    let agent_type_yaml = r#"
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
"#;

    #[cfg(target_family = "windows")]
    let agent_type_yaml = r#"
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
        path: powershell
        args: '-Command "Start-Sleep -Seconds ${nr-var:duration-1}"'
        restart_policy:
          backoff_strategy:
            type: fixed
            max_retries: 2
            backoff_delay: 0s
      - id: sleep2
        path: powershell
        args: '-Command "Start-Sleep -Seconds ${nr-var:duration-2}"'
        restart_policy:
          backoff_strategy:
            type: fixed
            max_retries: 2
            backoff_delay: 0s
"#;

    let _agent_type_def = create_temp_file(
        dir.path().join(DYNAMIC_AGENT_TYPE_DIR).as_path(),
        "test-agent.yaml",
        agent_type_yaml,
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
        // cmd_assert is not made for long running programs, so we kill it anyway after some time
        cmd.timeout(Duration::from_secs(20));
        // But in any case we make sure that it actually attempted to create the supervisor group,
        // so it works when the program is run as root
        let logs = cmd.output().expect("failed to execute process").stdout;
        println!("{}", String::from_utf8(logs).unwrap());
    });

    thread::sleep(Duration::from_secs(10));

    // Find the process id of both sleep/timeout commands
    // It is expected that only one such process is found!
    #[cfg(target_family = "unix")]
    let pattern_1 = "sleep 1000000";
    #[cfg(target_family = "windows")]
    let pattern_1 = "Start-Sleep -Seconds 1000000";

    let pids = find_processes_by_pattern(pattern_1);
    assert!(!pids.is_empty(), "First process should be running");
    let sleep_pid = pids[0].clone();

    #[cfg(target_family = "unix")]
    let pattern_2 = "sleep 2000000";
    #[cfg(target_family = "windows")]
    let pattern_2 = "Start-Sleep -Seconds 2000000";

    // Send a SIGKILL to both yes commands
    let pids = find_processes_by_pattern(pattern_2);
    assert!(!pids.is_empty(), "Second process should be running");
    let second_sleep_pid = pids[0].clone();

    // Kill both processes
    #[cfg(target_family = "unix")]
    {
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
    }

    #[cfg(target_family = "windows")]
    {
        use std::process::Command;
        Command::new("taskkill")
            .args(["/F", "/PID", &sleep_pid])
            .output()
            .expect("failed to kill first process");
        Command::new("taskkill")
            .args(["/F", "/PID", &second_sleep_pid])
            .output()
            .expect("failed to kill second process");
    }

    // Wait for the agent-control to restart the process
    thread::sleep(Duration::from_secs(2));

    // Get the pid for both new processes
    let new_pids = find_processes_by_pattern(pattern_1);
    assert!(!new_pids.is_empty(), "First process should have restarted");
    let new_sleep_pid = &new_pids[0];

    let new_pids_2 = find_processes_by_pattern(pattern_2);
    assert!(
        !new_pids_2.is_empty(),
        "Second process should have restarted"
    );
    let new_second_sleep_pid = &new_pids_2[0];

    agent_control_join.join().unwrap();

    // Assert the PIDs are different
    assert_ne!(sleep_pid.trim(), new_sleep_pid.trim());
    assert_ne!(second_sleep_pid.trim(), new_second_sleep_pid.trim());

    Ok(())
}
