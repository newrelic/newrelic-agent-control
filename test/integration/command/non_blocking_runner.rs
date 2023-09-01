use std::process::Command;

use newrelic_super_agent::command::{
    CommandExecutor, CommandHandle, CommandTerminator, ProcessRunner, ProcessTerminator,
};

// non blocking supervisor
struct NonSupervisor<C = ProcessRunner>
where
    C: CommandExecutor,
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
        cmd: ProcessRunner::new("sleep", ["5"], HashMap::from([("TEST", "TEST")])),
    };

    let started_cmd = agent.cmd.start().unwrap();

    // kill the process
    let terminated = ProcessTerminator::new(started_cmd.get_pid()).shutdown(|| true);
    assert!(terminated.is_ok());
}
