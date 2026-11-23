use newrelic_agent_control::sub_agent::on_host::managed_process::ProcessOrigin;
#[cfg(target_family = "unix")]
use newrelic_agent_control::sub_agent::on_host::managed_process::ProcessState;
use newrelic_agent_control::sub_agent::on_host::managed_process::spawned_process::executable_data::ExecutableData;
use newrelic_agent_control::sub_agent::on_host::managed_process::spawned_process::os_process::OsProcessConfig;
use newrelic_agent_control::sub_agent::on_host::managed_process::spawned_process::restart_policy::RestartPolicy;
use std::collections::HashMap;
use std::{thread::sleep, time::Duration};

#[test]
fn non_blocking_runner() {
    let agent_id = "sleep-test".to_string().try_into().unwrap();

    let config = OsProcessConfig::new(
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
        None,
    );

    let started_cmd = ProcessOrigin::Spawn(config).try_into_supervised().unwrap();
    sleep(Duration::from_millis(500)); // Give the process some room to start

    let terminated = started_cmd.shutdown();
    assert!(terminated.is_ok());
}

#[test]
#[cfg(target_family = "unix")]
fn command_shutdown_when_sigterm_is_ignored() {
    let agent_id = "test".to_string().try_into().unwrap();
    let cmd = ProcessOrigin::Spawn(OsProcessConfig::new(
        agent_id,
        &ExecutableData {
            id: "test".to_string(),
            bin: "tests/on_host/data/ignore_sigterm.sh".to_string(),
            args: Default::default(),
            env: Default::default(),
            restart_policy: Default::default(),
            shutdown_timeout: Duration::from_millis(100),
        },
        None,
    ))
    .try_into_supervised()
    .unwrap();

    // Confirm the process is running before sending SIGTERM.
    let ProcessState::Running(cmd) = cmd.try_wait() else {
        panic!("process exited before shutdown test could run")
    };

    // shutdown() sends SIGTERM, waits shutdown_timeout, then SIGKILL, and only returns once
    // the process is confirmed dead — no post-shutdown polling needed.
    assert!(cmd.shutdown().is_ok());
}

#[test]
#[cfg(target_family = "windows")]
fn command_shutdown_kill_orphan_process() {
    use crate::common::retry::retry;
    use crate::on_host::tools::windows_process::{is_process_orphan, is_process_running};

    let agent_id = "test".to_string().try_into().unwrap();
    let id = jiff::Zoned::now().strftime("%Y%m%d_%H%M%S").to_string();
    let cmd = ProcessOrigin::Spawn(OsProcessConfig::new(
        agent_id,
        &ExecutableData {
            id: "test".to_string(),
            bin: "powershell".to_string(),
            args: vec![
                "-NoProfile".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-File".to_string(),
                "tests\\on_host\\data\\leak_sub_process.ps1".to_string(),
                id.clone(),
            ],
            env: Default::default(),
            restart_policy: Default::default(),
            shutdown_timeout: Duration::from_millis(100),
        },
        None,
    ))
    .try_into_supervised()
    .unwrap();

    // Check that we have a leaked process
    retry(30, Duration::from_secs(1), || {
        if is_process_running(&id) && is_process_orphan(&id) {
            Ok(())
        } else {
            Err("Process not running or not orphaned yet".into())
        }
    });

    cmd.shutdown().unwrap();

    retry(30, Duration::from_secs(1), || {
        if !is_process_running(&id) {
            Ok(())
        } else {
            Err("Orphan process leaked".into())
        }
    });
}

#[test]
#[cfg(target_family = "unix")]
fn command_shutdown_sigterm_reaches_descendants() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("grandchild_terminated");

    let cmd = ProcessOrigin::Spawn(OsProcessConfig::new(
        "test".to_string().try_into().unwrap(),
        &ExecutableData {
            id: "test".to_string(),
            bin: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/on_host/data/term_propagation.sh")
                .to_string_lossy()
                .into_owned(),
            args: vec![marker.to_string_lossy().into_owned()],
            env: Default::default(),
            restart_policy: Default::default(),
            shutdown_timeout: Duration::from_millis(500),
        },
        None,
    ))
    .try_into_supervised()
    .unwrap();

    // Give the script time to install its traps and fork the grandchild.
    sleep(Duration::from_millis(500));

    // The root process ignores SIGTERM and doesn't forward it, so the marker can only be
    // written if the signal was delivered to the whole process group.
    assert!(cmd.shutdown().is_ok());

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !marker.exists() && std::time::Instant::now() < deadline {
        sleep(Duration::from_millis(50));
    }
    assert!(marker.exists(), "grandchild did not receive SIGTERM");
}
