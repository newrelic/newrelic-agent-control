use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::file::write;
use crate::common::logs::{clear_logs, expect_log_line_contains};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::windows::install::{SERVICE_NAME, install_agent_control_from_recipe, tear_down_test};
use crate::windows::service::{STATUS_RUNNING, restart_service, stop_service};
use crate::windows::{self, DEFAULT_LOG_PATH};
use std::fs;
use std::path::Path;
use std::time::Duration;
use tracing::info;

/// Same fixture used by the `shutdown` integration tests.
const CTRL_BREAK_SCRIPT: &str =
    include_str!("../../../../../agent-control/tests/on_host/data/ctrl_break_handler.ps1");

const WORK_DIR: &str = r"C:\ctrl-break-e2e";
const DYNAMIC_AGENT_TYPES_DIR: &str =
    r"C:\Program Files\New Relic\newrelic-agent-control\dynamic-agent-types";
const AGENT_TYPE: &str = "external/com.newrelic.e2e.ctrlbreak:0.0.1";
const HANDLER_AGENT_ID: &str = "ctrl-break-handler";
const IGNORER_AGENT_ID: &str = "ctrl-break-ignorer";

const AGENT_TYPE_DEFINITION: &str = r#"
namespace: external
name: com.newrelic.e2e.ctrlbreak
version: 0.0.1
protocol_version: "1.0"
platform: host
operating_system: windows
variables:
  script:
    description: "Path of the script that installs the CTRL_BREAK handler"
    type: string
    required: true
  dir:
    description: "Directory where the script writes its ready and got_break markers"
    type: string
    required: true
  mode:
    description: "Either handle or ignore"
    type: string
    required: true
deployment:
  executables:
    - id: ctrl-break
      path: 'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe'
      args:
        - -NoProfile
        - -ExecutionPolicy
        - Bypass
        - -File
        - ${nr-var:script}
        - ${nr-var:dir}
        - ${nr-var:mode}
      restart_policy:
        backoff_strategy:
          type: fixed
          backoff_delay: 20s
"#;

/// Verifies that stopping Agent Control as a Windows service delivers `CTRL_BREAK_EVENT` to its
/// sub-agents. Unlike a console session, a service has no console attached, so this checks the
/// signal still reaches the sub-agents' process groups:
/// - a sub-agent that handles `CTRL_BREAK_EVENT` receives it and exits before the shutdown timeout;
/// - a sub-agent that ignores it is only force-killed after the shutdown timeout.
pub fn test_ctrl_break_shutdown(args: InstallationArgs) {
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(|| {
        let _ = fs::remove_dir_all(WORK_DIR);
        let _ = fs::remove_dir_all(DYNAMIC_AGENT_TYPES_DIR);
        tear_down_test();
    });

    install_agent_control_from_recipe(&recipe_data);

    let script = Path::new(WORK_DIR).join("ctrl_break_handler.ps1");
    let handler_dir = Path::new(WORK_DIR).join("handler");
    let ignorer_dir = Path::new(WORK_DIR).join("ignorer");
    for dir in [&handler_dir, &ignorer_dir] {
        fs::create_dir_all(dir).expect("should create the sub-agent work dir");
    }
    write(&script, CTRL_BREAK_SCRIPT);

    fs::create_dir_all(DYNAMIC_AGENT_TYPES_DIR).expect("should create the dynamic agent types dir");
    write(
        Path::new(DYNAMIC_AGENT_TYPES_DIR).join("ctrl-break.yaml"),
        AGENT_TYPE_DEFINITION,
    );

    update_config(
        windows::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
agents:
  {HANDLER_AGENT_ID}:
    agent_type: "{AGENT_TYPE}"
  {IGNORER_AGENT_ID}:
    agent_type: "{AGENT_TYPE}"
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );
    for (agent_id, dir, mode) in [
        (HANDLER_AGENT_ID, &handler_dir, "handle"),
        (IGNORER_AGENT_ID, &ignorer_dir, "ignore"),
    ] {
        write_agent_local_config(
            &windows::local_config_path(agent_id),
            format!(
                "script: '{}'\ndir: '{}'\nmode: {mode}\n",
                script.display(),
                dir.display()
            ),
        );
    }

    restart_service(SERVICE_NAME, STATUS_RUNNING);

    // The handler must be installed before the service is stopped, otherwise the event is lost.
    for dir in [&handler_dir, &ignorer_dir] {
        retry_panic(
            30,
            Duration::from_secs(2),
            "sub-agent CTRL_BREAK handler installed",
            || {
                if dir.join("ready").exists() {
                    Ok(())
                } else {
                    Err(format!("{} not ready yet", dir.display()).into())
                }
            },
        );
    }

    clear_logs(DEFAULT_LOG_PATH).expect("should clear logs to avoid wrong match hits");

    info!("Stopping the Agent Control service");
    stop_service(SERVICE_NAME);

    assert!(
        handler_dir.join("got_break").exists(),
        "the sub-agent that handles CTRL_BREAK did not receive it from the service"
    );
    assert!(
        !ignorer_dir.join("got_break").exists(),
        "the sub-agent that ignores CTRL_BREAK should not have exited through its handler"
    );

    // Agent Control logs the forced stop for the sub-agent that outlived the shutdown timeout, and
    // nothing for the one that exited gracefully.
    retry_panic(
        10,
        Duration::from_secs(1),
        "forced stop log line of the ignoring sub-agent",
        || {
            expect_log_line_contains(
                DEFAULT_LOG_PATH,
                &["Graceful shutdown timeout exceeded", IGNORER_AGENT_ID],
            )
        },
    );
    retry_panic(
        10,
        Duration::from_secs(1),
        "graceful exit log line of the handling sub-agent",
        || {
            expect_log_line_contains(
                DEFAULT_LOG_PATH,
                &[
                    "Process exited after the graceful shutdown signal",
                    HANDLER_AGENT_ID,
                ],
            )
        },
    );
    expect_log_line_contains(
        DEFAULT_LOG_PATH,
        &["Graceful shutdown signal sent", IGNORER_AGENT_ID],
    )
    .expect("the signal should have been sent to the ignoring sub-agent too");
    for forced_stop in [
        "Graceful shutdown failed",
        "Graceful shutdown timeout exceeded",
    ] {
        assert!(
            expect_log_line_contains(DEFAULT_LOG_PATH, &[forced_stop, HANDLER_AGENT_ID]).is_err(),
            "the sub-agent that handles CTRL_BREAK must not be stopped forcefully: {forced_stop}"
        );
    }

    info!(
        "Test passed: CTRL_BREAK is delivered to sub-agents when Agent Control runs as a service"
    );
}
