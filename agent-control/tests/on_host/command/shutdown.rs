use newrelic_agent_control::sub_agent::on_host::command::command_os::CommandOSNotStarted;
use newrelic_agent_control::sub_agent::on_host::command::executable_data::ExecutableData;
use newrelic_agent_control::sub_agent::on_host::command::restart_policy::RestartPolicy;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::{thread::sleep, time::Duration};

#[test]
fn non_blocking_runner() {
    let agent_id = "sleep-test".to_string().try_into().unwrap();
    let mut sleep_cmd = Command::new("sleep");
    sleep_cmd.arg("5");

    let cmd = CommandOSNotStarted::new(
        agent_id,
        #[cfg(target_family = "unix")]
        &ExecutableData {
            id: "sleep".to_string(),
            bin: "sleep".to_string(),
            args: vec!["5".to_string()],
            env: HashMap::from([("TEST".to_string(), "TEST".to_string())]),
            restart_policy: RestartPolicy::default(),
            shutdown_timeout: Default::default(),
        },
        #[cfg(target_family = "windows")]
        &ExecutableData {
            id: "sleep".to_string(),
            bin: "powershell".to_string(),
            args: vec!["-Command".to_string(), "Start-Sleep -Seconds 5".to_string()],
            env: HashMap::from([("TEST".to_string(), "TEST".to_string())]),
            restart_policy: RestartPolicy::default(),
            shutdown_timeout: Default::default(),
        },
        false,
        PathBuf::default(),
    );

    let mut started_cmd = cmd.start().unwrap();
    sleep(Duration::from_millis(500)); // Give the process some room to start

    let terminated = started_cmd.shutdown();
    assert!(terminated.is_ok());
}

#[test]
fn command_shutdown_when_sigterm_is_ignored() {
    let agent_id = "test".to_string().try_into().unwrap();
    let mut cmd = CommandOSNotStarted::new(
        agent_id,
        #[cfg(target_family = "unix")]
        &ExecutableData {
            id: "test".to_string(),
            bin: "tests/on_host/data/ignore_sigterm.sh".to_string(),
            args: Default::default(),
            env: Default::default(),
            restart_policy: Default::default(),
            shutdown_timeout: Duration::from_millis(100),
        },
        #[cfg(target_family = "windows")]
        &ExecutableData {
            id: "test".to_string(),
            bin: "powershell".to_string(),
            args: vec![
                "-NoProfile".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-File".to_string(),
                "tests\\on_host\\data\\ignore_sigbreak.ps1".to_string(),
            ],
            env: Default::default(),
            restart_policy: Default::default(),
            shutdown_timeout: Duration::from_millis(100),
        },
        false,
        Default::default(),
    )
    .start()
    .unwrap();
    sleep(Duration::from_millis(500)); // Give the process some room to start

    let terminated = cmd.shutdown();
    sleep(Duration::from_millis(200));
    assert!(!cmd.is_running());
    assert!(terminated.is_ok());
}
