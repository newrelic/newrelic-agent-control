//! Scenarios of the experimental cgroup tracking of on-host executables.
//!
//! Except for the first one they write to the cgroup filesystem, so they are ignored by default
//! and need a delegated cgroup. Run them with `make -C agent-control test/onhost/cgroups/integration`,
//! which wraps the test binary in a delegated scope. They deliberately avoid the `_as_root` suffix:
//! the root test job has no delegated scope and writing cgroups there can confuse systemd.

use crate::common::agent_control::{StartedAgentControl, start_agent_control_with_custom_config};
use crate::common::base_paths::TempBasePaths;
use crate::common::retry::retry;
use crate::common::runtime::tokio_runtime;
use crate::on_host::tools::config::{OnHostAgentControlConfigBuilder, create_local_config};
use crate::on_host::tools::custom_agent_type::OnHostCustomAgentTypeBuilder;
use crate::on_host::tools::instance_id::get_instance_id;
use cgroup_tracker::{CgroupName, CgroupTracker};
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;

const AGENT_ID: &str = "test-agent";
const EXEC_ID: &str = "test-executable";
const FIRST_EXEC_ID: &str = "first-executable";
const SECOND_EXEC_ID: &str = "second-executable";

const LONG_RUNNING: &str = "exec sleep 3600";
/// Writes its pid to `{pid_dir}/{exec_id}.pid`, see [`Scenario::pid_of`].
const PID_FILE_SLEEP: &str = "echo $$ > {pid_dir}/{exec_id}.pid; exec sleep 3600";
/// Leaves a `setsid` child that is out of the executable's process group but still in its cgroup.
const DETACHED_SLEEP: &str = "setsid sleep 3600 & wait";

const RESTART_POLICY_YAML: &str =
    "    backoff_strategy:\n      type: fixed\n      max_retries: 2\n      backoff_delay: 0s\n";

/// An agent control running one sub-agent with the given `(exec_id, script)` executables.
///
/// Each executable runs its script with `sh -c`. In the script, `{pid_dir}` is replaced with a
/// directory that belongs to the scenario and `{exec_id}` with the id of the executable, so the
/// script can leave files (such as its pid) for the test to read.
struct Scenario {
    _dirs: TempBasePaths,
    _opamp_server: FakeServer,
    pid_dir: TempDir,
    agent_control: Option<StartedAgentControl>,
}

impl Scenario {
    fn start(executables: &[(&str, &str)], cgroups_enabled: bool) -> Self {
        let opamp_server = FakeServer::start(tokio_runtime().handle());
        let dirs = TempBasePaths::default();
        let pid_dir = TempDir::new().unwrap();
        let executables_yaml: String = executables
            .iter()
            .map(|(exec_id, script)| {
                let script = script
                    .replace("{pid_dir}", &pid_dir.path().to_string_lossy())
                    .replace("{exec_id}", exec_id);
                format!(
                    "- id: {exec_id}\n  path: sh\n  args:\n    - \"-c\"\n    - \"{script}\"\n  restart_policy:\n{}",
                    RESTART_POLICY_YAML
                )
            })
            .collect();
        let agent_type = OnHostCustomAgentTypeBuilder::empty()
            .with_executables(Some(&executables_yaml))
            .write(dirs.local_dir());
        let mut config = OnHostAgentControlConfigBuilder::new(
            opamp_server.endpoint(),
            opamp_server.jwks_endpoint(),
        )
        .with_agent(AGENT_ID, agent_type);
        if cgroups_enabled {
            config = config.with_cgroups_enabled();
        }
        config.write(dirs.local_dir());
        create_local_config(
            AGENT_ID.to_string(),
            "fake_variable: value".to_string(),
            dirs.local_dir(),
        );
        let agent_control =
            start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);
        Self {
            _dirs: dirs,
            _opamp_server: opamp_server,
            pid_dir,
            agent_control: Some(agent_control),
        }
    }

    fn pid_file(&self, name: &str) -> PathBuf {
        self.pid_dir.path().join(name)
    }

    /// Pid written by an executable running [`PID_FILE_SLEEP`].
    fn pid_of(&self, exec_id: &str) -> i32 {
        wait_for_pid_file(&self.pid_file(&format!("{exec_id}.pid")))
    }

    fn stop(&mut self) {
        drop(self.agent_control.take());
    }
}

/// The cgroup Agent Control creates for an executable of [`AGENT_ID`].
struct ExecutableCgroup {
    tracker: CgroupTracker,
    name: CgroupName,
}

impl ExecutableCgroup {
    fn new(exec_id: &str) -> Self {
        Self {
            tracker: CgroupTracker::detect().unwrap(),
            name: CgroupName::try_from(format!("{AGENT_ID}_{exec_id}")).unwrap(),
        }
    }

    fn members(&self) -> Result<Vec<i32>, String> {
        self.tracker
            .cgroup(&self.name)
            .members()
            .map_err(|err| err.to_string())
    }

    /// Pids in the cgroup once it has at least `expected` of them.
    fn wait_for_members(&self, expected: usize) -> Vec<i32> {
        retry(30, Duration::from_secs(1), || {
            let members = self.members()?;
            if members.len() >= expected {
                Ok(members)
            } else {
                Err(format!("members: {members:?}").into())
            }
        })
    }

    /// Pid of the only process in the cgroup, once there is exactly one.
    fn wait_for_single_member(&self) -> i32 {
        retry(30, Duration::from_secs(1), || {
            let members = self.members()?;
            match members.as_slice() {
                [pid] => Ok(*pid),
                _ => Err(format!("members: {members:?}").into()),
            }
        })
    }

    /// Pid of the process that replaced `old_pid`, once it is the only one in the cgroup.
    fn wait_for_replacement_of(&self, old_pid: i32) -> i32 {
        retry(30, Duration::from_secs(1), || {
            let pid = self.wait_for_single_member();
            if pid == old_pid {
                Err(format!("process {old_pid} is still the only member").into())
            } else {
                Ok(pid)
            }
        })
    }

    fn wait_until_removed(&self) {
        wait_until("the cgroup to be removed", || {
            !self.tracker.cgroup(&self.name).path().exists()
        });
    }
}

fn wait_until(description: &str, mut condition: impl FnMut() -> bool) {
    retry(30, Duration::from_secs(1), || {
        if condition() {
            Ok(())
        } else {
            Err(format!("timed out waiting for {description}").into())
        }
    });
}

/// Pid a script wrote with `echo $$ > file`.
fn wait_for_pid_file(path: &Path) -> i32 {
    retry(30, Duration::from_secs(1), || {
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        content
            .trim()
            .parse::<i32>()
            .map_err(|e| e.to_string().into())
    })
}

fn is_alive(pid: i32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn cgroup_of(pid: i32) -> String {
    fs::read_to_string(format!("/proc/{pid}/cgroup"))
        .unwrap()
        .trim()
        .to_string()
}

#[test]
fn flag_off_creates_no_cgroups() {
    let scenario = Scenario::start(&[(EXEC_ID, PID_FILE_SLEEP)], false);

    let pid = scenario.pid_of(EXEC_ID);

    assert!(!cgroup_of(pid).contains("_nr-ac"));
}

#[test]
#[ignore = "needs a delegated cgroup, see the module docs"]
fn spawn_places_the_executable_in_its_cgroup() {
    let scenario = Scenario::start(&[(EXEC_ID, PID_FILE_SLEEP)], true);
    let cgroup = ExecutableCgroup::new(EXEC_ID);

    let member = cgroup.wait_for_single_member();

    let pid = scenario.pid_of(EXEC_ID);
    assert_eq!(member, pid);
    assert!(
        cgroup_of(pid).ends_with(&format!("_nr-ac/{AGENT_ID}_{EXEC_ID}")),
        "unexpected cgroup: {}",
        cgroup_of(pid)
    );
}

#[test]
#[ignore = "needs a delegated cgroup, see the module docs"]
fn stop_sweeps_descendants_that_left_the_process_group() {
    let mut scenario = Scenario::start(&[(EXEC_ID, DETACHED_SLEEP)], true);
    let cgroup = ExecutableCgroup::new(EXEC_ID);
    let members = cgroup.wait_for_members(2);

    scenario.stop();

    wait_until("every member to exit", || {
        members.iter().all(|pid| !is_alive(*pid))
    });
    cgroup.wait_until_removed();
}

#[test]
#[ignore = "needs a delegated cgroup, see the module docs"]
fn restart_places_the_new_process_in_the_cgroup() {
    let _scenario = Scenario::start(&[(EXEC_ID, LONG_RUNNING)], true);
    let cgroup = ExecutableCgroup::new(EXEC_ID);
    let old_pid = cgroup.wait_for_single_member();

    kill(Pid::from_raw(old_pid), Signal::SIGKILL).unwrap();

    let new_pid = cgroup.wait_for_replacement_of(old_pid);

    assert!(is_alive(new_pid));
    assert!(!is_alive(old_pid));
}

#[test]
#[ignore = "needs a delegated cgroup, see the module docs"]
fn an_executable_exiting_does_not_affect_the_others() {
    let _scenario = Scenario::start(
        &[
            (FIRST_EXEC_ID, LONG_RUNNING),
            (SECOND_EXEC_ID, LONG_RUNNING),
        ],
        true,
    );
    let first = ExecutableCgroup::new(FIRST_EXEC_ID);
    let second = ExecutableCgroup::new(SECOND_EXEC_ID);
    let first_pid = first.wait_for_single_member();
    let second_pid = second.wait_for_single_member();

    kill(Pid::from_raw(first_pid), Signal::SIGKILL).unwrap();

    first.wait_for_replacement_of(first_pid);
    assert_eq!(second.members().unwrap(), vec![second_pid]);
    assert!(is_alive(second_pid));
}
