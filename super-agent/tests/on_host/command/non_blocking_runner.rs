use std::process::Command;

use newrelic_super_agent::sub_agent::on_host::command::command::{
    CommandTerminator, NotStartedCommand, StartedCommand,
};
use newrelic_super_agent::sub_agent::on_host::command::command_os::{CommandOS, NotStarted};
use newrelic_super_agent::sub_agent::on_host::command::shutdown::ProcessTerminator;
use newrelic_super_agent::sub_agent::on_host::supervisor::executable_data::ExecutableData;
use newrelic_super_agent::sub_agent::on_host::supervisor::restart_policy::RestartPolicy;
use std::collections::HashMap;
use std::path::PathBuf;

// non blocking supervisor
struct NonSupervisor<C = CommandOS<NotStarted>>
where
    C: NotStartedCommand,
{
    cmd: C,
}

#[cfg(unix)]
#[test]
fn non_blocking_runner() {
    let agent_id = "sleep-test".to_string().try_into().unwrap();
    let mut sleep_cmd = Command::new("sleep");
    sleep_cmd.arg("5");

    let agent = NonSupervisor {
        cmd: CommandOS::<NotStarted>::new(
            agent_id,
            &ExecutableData {
                bin: "sleep".to_string(),
                args: vec!["5".to_string()],
                env: HashMap::from([("TEST".to_string(), "TEST".to_string())]),
                restart_policy: RestartPolicy::default(),
            },
            false,
            PathBuf::default(),
        ),
    };

    let started_cmd = agent.cmd.start().unwrap();

    // kill the process
    let terminated = ProcessTerminator::new(started_cmd.get_pid()).shutdown(|| true);
    assert!(terminated.is_ok());
}
