#![cfg(target_family = "unix")]
use newrelic_agent_control::sub_agent::on_host::command::command_os::CommandOSNotStarted;
use newrelic_agent_control::sub_agent::on_host::command::executable_data::ExecutableData;
use newrelic_agent_control::sub_agent::on_host::command::restart_policy::RestartPolicy;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn non_blocking_runner() {
    let agent_id = "sleep-test".to_string().try_into().unwrap();
    let mut sleep_cmd = Command::new("sleep");
    sleep_cmd.arg("5");

    let cmd = CommandOSNotStarted::new(
        agent_id,
        &ExecutableData {
            id: "sleep".to_string(),
            bin: "sleep".to_string(),
            args: vec!["5".to_string()],
            env: HashMap::from([("TEST".to_string(), "TEST".to_string())]),
            restart_policy: RestartPolicy::default(),
        },
        false,
        PathBuf::default(),
    );

    let mut started_cmd = cmd.start().unwrap();
    let terminated = started_cmd.shutdown();
    assert!(terminated.is_ok());
}
