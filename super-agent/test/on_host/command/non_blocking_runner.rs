use std::process::Command;

use newrelic_super_agent::sub_agent::on_host::command::command::{
    CommandTerminator, NotStartedCommand, StartedCommand,
};
use newrelic_super_agent::sub_agent::on_host::command::command_os::{CommandOS, NotStarted};
use newrelic_super_agent::sub_agent::on_host::command::shutdown::ProcessTerminator;

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
    use std::collections::HashMap;

    let mut sleep_cmd = Command::new("sleep");
    sleep_cmd.arg("5");

    let agent = NonSupervisor {
        cmd: CommandOS::<NotStarted>::new("sleep", ["5"], HashMap::from([("TEST", "TEST")])),
    };

    let started_cmd = agent.cmd.start().unwrap();

    // kill the process
    let terminated = ProcessTerminator::new(started_cmd.get_pid()).shutdown(|| true);
    assert!(terminated.is_ok());
}
