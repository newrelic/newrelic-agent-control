use std::process::Command;

use meta_agent::command::{wrapper::ProcessRunner, CommandExecutor, CommandHandle};

// non blocking supervisor
struct NonSupervisor<C = ProcessRunner>
where
    C: CommandExecutor,
{
    cmd: C,
}

#[test]
fn non_blocking_runner() {
    let mut sleep_cmd = Command::new("sleep");
    sleep_cmd.arg("5");

    let agent = NonSupervisor {
        cmd: ProcessRunner::new("sleep", ["5"]),
    };

    let started_cmd = agent.cmd.start().unwrap();

    // kill the process
    assert_eq!(started_cmd.stop().is_err(), false);
}
