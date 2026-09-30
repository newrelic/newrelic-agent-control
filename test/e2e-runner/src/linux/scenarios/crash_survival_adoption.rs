//! E2E scenario for the on-host crash-survival PoC: kills Agent Control's real main PID and
//! verifies that, under the packaged unit's `KillMode=process`, a sub-agent survives
//! unsupervised, and the restarted Agent Control adopts it instead of respawning a duplicate.
//!
//! This is the automated-in-CI equivalent of scripts/poc-validate-crash-survival.sh, which
//! remains as a manual entry point for validating the same behavior outside CI. See the
//! on-host crash-survival CDD/PoC for the full design rationale.

use crate::common::config::{update_config, write_agent_local_config};
use crate::common::file::write;
use crate::common::on_drop::CleanUp;
use crate::common::test::{TestResult, retry_panic};
use crate::common::{InstallationArgs, RecipeData};
use crate::linux;
use crate::linux::bash::exec_bash_command;
use crate::linux::install::{install_agent_control_from_recipe, tear_down_test};
use crate::linux::service::{get_main_pid, restart_service};
use std::time::Duration;
use tracing::info;

const DYNAMIC_AGENT_TYPE_PATH: &str =
    "/etc/newrelic-agent-control/dynamic-agent-types/crash-survival-poc.yaml";
const SUB_AGENT_ID: &str = "crash-survival-sleeper";
// An arbitrary, distinctive `sleep` duration used purely so `pgrep -f` can find this exact
// sub-agent process on a shared CI runner without matching an unrelated sleep invocation.
const SUB_AGENT_PROCESS_PATTERN: &str = "sleep 4863";

const CRASH_SURVIVAL_AGENT_TYPE: &str = r#"
namespace: newrelic
name: com.newrelic.crash_survival_poc
version: 0.1.0
platform: host
operating_system: linux
protocol_version: "1.0"
variables: {}
deployment:
  executables:
    - id: "crash-survival-sleeper"
      path: "/bin/sleep"
      args:
        - "4863"
  health:
    interval: 60s
    initial_delay: 0s
    timeout: 15s
    checks:
      - kind: Process
"#;

/// Finds a still-running process's PID by matching its full command line, or `None`.
fn find_pid_by_pattern(pattern: &str) -> TestResult<u32> {
    let output = exec_bash_command(&format!("pgrep -f '{pattern}' | head -n1"))?;
    output
        .lines()
        .find(|l| l.starts_with("Stdout: "))
        .and_then(|l| l.strip_prefix("Stdout: "))
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "no matching process found yet".into())
}

fn is_pid_alive(pid: u32) -> bool {
    exec_bash_command(&format!("kill -0 {pid}")).is_ok()
}

pub fn test_ac_crash_survival_and_process_adoption(args: InstallationArgs) {
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    info!("Registering the crash-survival sub-agent's custom agent type");
    let dynamic_agent_type_path = std::path::Path::new(DYNAMIC_AGENT_TYPE_PATH);
    std::fs::create_dir_all(dynamic_agent_type_path.parent().unwrap()).unwrap_or_else(|err| {
        panic!("could not create dynamic agent types directory: {err}");
    });
    write(dynamic_agent_type_path, CRASH_SURVIVAL_AGENT_TYPE);

    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
agents:
  {SUB_AGENT_ID}:
    agent_type: "newrelic/com.newrelic.crash_survival_poc:0.1.0"
"#
        ),
    );
    write_agent_local_config(&linux::local_config_path(SUB_AGENT_ID), String::new());

    restart_service(linux::SERVICE_NAME);

    info!("Waiting for Agent Control and the sub-agent to come up");
    let ac_pid_before =
        retry_panic(
            30,
            Duration::from_secs(2),
            "Agent Control main PID",
            || match get_main_pid(linux::SERVICE_NAME) {
                0 => Err("Agent Control is not running yet".into()),
                pid => Ok(pid),
            },
        );
    let sub_agent_pid = retry_panic(30, Duration::from_secs(2), "sub-agent PID", || {
        find_pid_by_pattern(SUB_AGENT_PROCESS_PATTERN)
    });
    info!(
        ac_pid_before,
        sub_agent_pid, "Agent Control and the sub-agent are both up"
    );

    info!(ac_pid_before, "Simulating a crash: kill -9 the main PID");
    exec_bash_command(&format!("kill -9 {ac_pid_before}"))
        .expect("should be able to signal Agent Control's main PID");

    // No retry here: under KillMode=process the sub-agent must never have been signaled at
    // all, so its survival is immediate, not something to wait for.
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        is_pid_alive(sub_agent_pid),
        "sub-agent PID {sub_agent_pid} did not survive Agent Control's crash \
         (KillMode=process regression: the rest of the cgroup was signaled too)"
    );
    info!(
        sub_agent_pid,
        "Sub-agent survived, unsupervised, as expected"
    );

    let ac_pid_after =
        retry_panic(
            40,
            Duration::from_secs(3),
            "Agent Control restart",
            || match get_main_pid(linux::SERVICE_NAME) {
                0 => Err("Agent Control has not restarted yet".into()),
                pid if pid == ac_pid_before => Err("still the pre-crash PID".into()),
                pid => Ok(pid),
            },
        );
    info!(ac_pid_after, "Agent Control restarted with a new main PID");

    retry_panic(20, Duration::from_secs(2), "adoption log line", || {
        exec_bash_command(&format!(
            "journalctl -u {} --since '2 minutes ago' | grep -qi 'adopted a still-running process'",
            linux::SERVICE_NAME
        ))
        .map_err(|_| "adoption log line not found yet".into())
    });
    info!("Confirmed the restarted Agent Control logged an adoption");

    let duplicate_count: u32 =
        exec_bash_command(&format!("pgrep -fc '{SUB_AGENT_PROCESS_PATTERN}' || true"))
            .ok()
            .and_then(|out| {
                out.lines()
                    .find(|l| l.starts_with("Stdout: "))
                    .and_then(|l| l.strip_prefix("Stdout: "))
                    .map(|s| s.trim().to_owned())
            })
            .and_then(|s| s.parse().ok())
            .expect("could not read pgrep -c output");
    assert_eq!(
        duplicate_count, 1,
        "expected exactly one sub-agent process; Agent Control likely respawned a duplicate \
         instead of adopting the existing one"
    );

    info!("Crash-survival-and-adoption scenario passed");
}
