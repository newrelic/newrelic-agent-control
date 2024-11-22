use newrelic_super_agent::sub_agent::on_host::command::command_os::CommandOSNotStarted;
use newrelic_super_agent::sub_agent::on_host::command::shutdown::ProcessTerminator;
use newrelic_super_agent::sub_agent::on_host::supervisor::executable_data::ExecutableData;
use newrelic_super_agent::sub_agent::on_host::supervisor::restart_policy::RestartPolicy;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

#[cfg(unix)]
#[test]
fn non_blocking_runner() {
    let agent_id = "sleep-test".to_string().try_into().unwrap();
    let mut sleep_cmd = Command::new("sleep");
    sleep_cmd.arg("5");

    let cmd = CommandOSNotStarted::new(
        agent_id,
        &ExecutableData {
            bin: "sleep".to_string(),
            args: vec!["5".to_string()],
            env: HashMap::from([("TEST".to_string(), "TEST".to_string())]),
            restart_policy: RestartPolicy::default(),
        },
        false,
        PathBuf::default(),
    );

    let started_cmd = cmd.start().unwrap();

    // kill the process
    let terminated = ProcessTerminator::new(started_cmd.get_pid()).shutdown(|| true);
    assert!(terminated.is_ok());
}
