use crate::common::retry::retry;
use crate::{
    common::super_agent::{init_sa, run_sa},
    on_host::cli::create_temp_file,
};
use assert_cmd::Command;
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};
use std::error::Error;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[cfg(unix)]
#[serial_test::serial]
#[test]
fn killing_subprocess_with_signal_restarts_as_root() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;

    let _agent_type_def = create_temp_file(
        &dir,
        "nrsa_local/dynamic-agent-type.yaml",
        r#"
namespace: newrelic
name: com.newrelic.test-agent
version: 0.0.1
variables:
  on_host:
    message:
      description: "Message to repeatedly output"
      type: string
      required: false
      default: "yes"
    file_logging:
      description: "Enable file logging"
      type: bool
      required: false
      default: false
deployment:
  on_host:
    enable_file_logging: "${nr-var:file_logging}"
    executables:
      - path: /usr/bin/yes
        args: "${nr-var:message}"
        restart_policy:
          backoff_strategy:
            type: fixed
            max_retries: 2
            backoff_delay: 0s
"#,
    );

    let _values_file = create_temp_file(
        &dir,
        "nrsa_local/fleet/agents.d/test-agent/values/values.yaml",
        r#"
message: "test yes"
file_logging: true
"#,
    );

    let (sa_cfg, _guard) = init_sa(
        dir.path(),
        r#"
log:
  level: info
  file:
    enable: true
agents:
  test-agent:
    agent_type: newrelic/com.newrelic.test-agent:0.0.1
host_id: test
"#,
    );

    let _ = thread::spawn(move || run_sa(sa_cfg));

    let mut yes_pid_old = String::new();
    retry(10, Duration::from_secs(1), || {
        || -> Result<(), Box<dyn Error>> {
            // Use `pgrep` to find the process id of the yes command
            // It is expected that only one such process is found!
            let yes_pid = Command::new("pgrep")
                .arg("-f")
                .arg("/usr/bin/yes test yes")
                .output()
                .expect("failed to execute process")
                .stdout;

            yes_pid_old = String::from_utf8(yes_pid)?;

            if yes_pid_old.is_empty() {
                return Err("command not found".into());
            }

            Ok(())
        }()
    });

    // Send a SIGKILL to the yes command
    signal::kill(
        Pid::from_raw(yes_pid_old.to_owned().trim().parse::<i32>().unwrap()),
        Signal::SIGKILL,
    )?;

    let mut yes_pid_new = String::new();
    retry(10, Duration::from_secs(1), || {
        || -> Result<(), Box<dyn Error>> {
            // Get the pid for the new yes command
            let new_yes_pid = Command::new("pgrep")
                .arg("-f")
                .arg("/usr/bin/yes test yes")
                .output()
                .expect("failed to execute process")
                .stdout;

            yes_pid_new = String::from_utf8(new_yes_pid)?;

            if yes_pid_new.is_empty() {
                return Err("command not found".into());
            }

            Ok(())
        }()
    });

    // Assert the PID is different
    assert_ne!(yes_pid_old, yes_pid_new);

    Ok(())
}
