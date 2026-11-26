use newrelic_agent_control::sub_agent::on_host::managed_process::ProcessOrigin;
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
    let id = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
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

#[cfg(target_family = "windows")]
mod ctrl_break {
    use super::*;
    use crate::common::retry::retry;
    use newrelic_agent_control::sub_agent::on_host::managed_process::SupervisedProcess;
    use std::path::Path;
    use std::time::Instant;

    const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(20);

    fn spawn_ctrl_break_child(
        dir: &Path,
        mode: &str,
        shutdown_timeout: Duration,
    ) -> SupervisedProcess {
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/on_host/data/ctrl_break_handler.ps1");
        let process = ProcessOrigin::Spawn(OsProcessConfig::new(
            "test".to_string().try_into().unwrap(),
            &ExecutableData {
                id: "test".to_string(),
                bin: "powershell".to_string(),
                args: vec![
                    "-NoProfile".to_string(),
                    "-ExecutionPolicy".to_string(),
                    "Bypass".to_string(),
                    "-File".to_string(),
                    script.display().to_string(),
                    dir.display().to_string(),
                    mode.to_string(),
                ],
                env: Default::default(),
                restart_policy: Default::default(),
                shutdown_timeout,
            },
            None,
        ))
        .try_into_supervised()
        .unwrap();

        // The handler must be installed before shutdown, otherwise the event is lost.
        retry(60, Duration::from_millis(500), || {
            if dir.join("ready").exists() {
                Ok(())
            } else {
                Err("Handler not installed yet".into())
            }
        });
        process
    }

    #[test]
    fn shutdown_delivers_ctrl_break_and_returns_before_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let process = spawn_ctrl_break_child(dir.path(), "handle", SHUTDOWN_TIMEOUT);

        let start = Instant::now();
        process.shutdown().unwrap();

        assert!(
            dir.path().join("got_break").exists(),
            "the child did not receive CTRL_BREAK_EVENT"
        );
        assert!(
            start.elapsed() < SHUTDOWN_TIMEOUT,
            "graceful shutdown should not wait for the timeout"
        );
    }

    #[test]
    fn shutdown_force_kills_when_ctrl_break_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let process = spawn_ctrl_break_child(dir.path(), "ignore", SHUTDOWN_TIMEOUT);

        let start = Instant::now();
        process.shutdown().unwrap();

        assert!(!dir.path().join("got_break").exists());
        assert!(
            start.elapsed() >= SHUTDOWN_TIMEOUT,
            "the process should be force killed only after the timeout"
        );
    }

    #[test]
    fn shutdown_ctrl_break_reaches_descendants() {
        let dir = tempfile::tempdir().unwrap();
        let process =
            spawn_ctrl_break_child(dir.path(), "ignore_with_child", Duration::from_secs(2));

        // The root ignores CTRL_BREAK, so the marker can only be written if the event was
        // delivered to the whole process group.
        process.shutdown().unwrap();

        let marker = dir.path().join("child").join("got_break");
        retry(10, Duration::from_millis(500), || {
            if marker.exists() {
                Ok(())
            } else {
                Err("Descendant did not receive CTRL_BREAK".into())
            }
        });
    }

    /// All processes share Agent Control's console, but `CTRL_BREAK` only reaches the group it is
    /// sent to.
    #[test]
    fn shutdown_ctrl_break_does_not_reach_other_processes() {
        let dir = tempfile::tempdir().unwrap();
        let (dir_a, dir_b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::create_dir(&dir_a).unwrap();
        std::fs::create_dir(&dir_b).unwrap();
        let process_a = spawn_ctrl_break_child(&dir_a, "handle", SHUTDOWN_TIMEOUT);
        let process_b = spawn_ctrl_break_child(&dir_b, "handle", SHUTDOWN_TIMEOUT);

        process_a.shutdown().unwrap();
        assert!(dir_a.join("got_break").exists());

        // B handles CTRL_BREAK by exiting, so it must still be running and not have seen it.
        let process_b = match process_b.try_wait() {
            ProcessState::Running(process) => process,
            ProcessState::Exited(status) => panic!("the other process exited: {status}"),
        };
        assert!(!dir_b.join("got_break").exists());

        process_b.shutdown().unwrap();
        assert!(dir_b.join("got_break").exists());
    }

    /// Every executable of every sub-agent is stopped from its own supervisor thread, so many
    /// `shutdown` calls signal at once. Prints how long each of them took.
    #[test]
    fn concurrent_shutdowns_deliver_ctrl_break_to_every_process() {
        const AGENTS: usize = 16;
        let dir = tempfile::tempdir().unwrap();
        let dirs: Vec<_> = (0..AGENTS)
            .map(|i| {
                let d = dir.path().join(i.to_string());
                std::fs::create_dir(&d).unwrap();
                d
            })
            .collect();

        // Like the supervisor, each thread spawns and stops its own process.
        let barrier = std::sync::Barrier::new(AGENTS);
        let start = Instant::now();
        let durations: Vec<Duration> = std::thread::scope(|scope| {
            let threads: Vec<_> = dirs
                .iter()
                .map(|d| {
                    let barrier = &barrier;
                    scope.spawn(move || {
                        let process = spawn_ctrl_break_child(d, "handle", SHUTDOWN_TIMEOUT);
                        barrier.wait();
                        let start = Instant::now();
                        process.shutdown().unwrap();
                        start.elapsed()
                    })
                })
                .collect();
            threads.into_iter().map(|h| h.join().unwrap()).collect()
        });
        println!(
            "{AGENTS} agents spawned and stopped in {:?}, per shutdown: min {:?}, max {:?}",
            start.elapsed(),
            durations.iter().min().unwrap(),
            durations.iter().max().unwrap(),
        );

        for d in &dirs {
            assert!(
                d.join("got_break").exists(),
                "{} did not receive CTRL_BREAK_EVENT",
                d.display()
            );
        }
        assert!(
            durations.iter().all(|d| *d < SHUTDOWN_TIMEOUT),
            "no shutdown should have waited for the timeout: {durations:?}"
        );
    }
}
