use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::opamp::FakeServer;
use crate::common::process_finder::find_processes_by_pattern;
use crate::common::retry::retry;
use crate::on_host::tools::config::{create_agent_control_config, create_local_config};
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use newrelic_agent_control::agent_control::run::{BasePaths, Environment};
use std::thread;
use std::time::Duration;
use tempfile::tempdir;

#[cfg(target_family = "unix")]
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};

/// The agent control is configured with no OpAMP and a custom agent-type to check that
/// a process gets restarted as expected when killing it externally.
#[test]
fn killing_subprocess_with_signal_restarts() -> Result<(), Box<dyn std::error::Error>> {
    let opamp_server = FakeServer::start_new();
    let local_dir = tempdir()?;
    let remote_dir = tempdir()?;

    // Create a custom agent type with long-running sleep processes
    let agent_type_builder = CustomAgentType::empty().with_variables(
        r#"
duration-1:
  description: "Duration for first sleep command"
  type: "string"
  required: true
duration-2:
  description: "Duration for second sleep command"
  type: "string"
  required: true
"#,
    );

    #[cfg(target_family = "unix")]
    let agent_type_builder = agent_type_builder.with_executables(Some(
        r#"
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
    ));

    #[cfg(target_family = "windows")]
    let agent_type_builder = agent_type_builder.with_executables(Some(
        r#"
- id: sleep1
  path: powershell
  args: "-NoProfile -ExecutionPolicy Bypass -File tests\\on_host\\data\\parameterized_sleep.ps1 -TimeoutSeconds ${nr-var:duration-1}"
  restart_policy:
    backoff_strategy:
      type: fixed
      max_retries: 2
      backoff_delay: 0s
- id: sleep2
  path: powershell
  args: "-NoProfile -ExecutionPolicy Bypass -File tests\\on_host\\data\\parameterized_sleep.ps1 -TimeoutSeconds ${nr-var:duration-2}"
  restart_policy:
    backoff_strategy:
      type: fixed
      max_retries: 2
      backoff_delay: 0s
"#,
    ));

    let agent_type = agent_type_builder.build(local_dir.path().to_path_buf());

    let agents = format!(
        r#"
agents:
  test-agent:
    agent_type: "{agent_type}"
"#
    );

    create_agent_control_config(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        agents,
        local_dir.path().to_path_buf(),
    );

    // Create local config to trigger the executable launch
    create_local_config(
        "test-agent".to_string(),
        r#"duration-1: "1000000"
duration-2: "2000000"
"#
        .to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    // Start Agent Control
    let _agent_control = start_agent_control_with_custom_config(base_paths, Environment::OnHost);

    // Find the process id of both sleep/timeout commands
    // It is expected that only one such process is found!
    #[cfg(target_family = "unix")]
    let pattern_1 = "sleep 1000000";
    #[cfg(target_family = "windows")]
    let pattern_1 = "-TimeoutSeconds 1000000";

    let sleep_pid = retry(15, Duration::from_secs(1), || {
        let pids = find_processes_by_pattern(pattern_1);
        if pids.is_empty() {
            Err("PID for first sleep process not found".into())
        } else {
            // I know there's at least one element at this point so I just unwrap
            Ok(pids.into_iter().next().unwrap())
        }
    });

    #[cfg(target_family = "unix")]
    let pattern_2 = "sleep 2000000";
    #[cfg(target_family = "windows")]
    let pattern_2 = "-TimeoutSeconds 2000000";

    let second_sleep_pid = retry(15, Duration::from_secs(1), || {
        let pids = find_processes_by_pattern(pattern_2);
        if pids.is_empty() {
            Err("PID for second sleep process not found".into())
        } else {
            // I know there's at least one element at this point so I just unwrap
            Ok(pids.into_iter().next().unwrap())
        }
    });

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
    let new_sleep_pid = retry(15, Duration::from_secs(1), || {
        let new_pids = find_processes_by_pattern(pattern_1);
        if new_pids.is_empty() {
            Err("First process should have been restarted".into())
        } else {
            // I know there's at least one element at this point so I just unwrap
            Ok(new_pids.into_iter().next().unwrap())
        }
    });

    let new_second_sleep_pid = retry(15, Duration::from_secs(1), || {
        let new_pids_2 = find_processes_by_pattern(pattern_2);
        if new_pids_2.is_empty() {
            Err("Second process should have been restarted".into())
        } else {
            // I know there's at least one element at this point so I just unwrap
            Ok(new_pids_2.into_iter().next().unwrap())
        }
    });

    // Assert the PIDs are different
    assert_ne!(sleep_pid.trim(), new_sleep_pid.trim());
    assert_ne!(second_sleep_pid.trim(), new_second_sleep_pid.trim());

    Ok(())
}
