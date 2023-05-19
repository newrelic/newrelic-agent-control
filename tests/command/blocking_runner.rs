use std::process::Command;

use meta_agent::command::{wrapper::ProcessRunner, CommandRunner};

// blocking supervisor
struct BlockingSupervisor<C = ProcessRunner>
where
    C: CommandRunner,
{
    cmd: C,
}

#[test]
fn blocking_stop_runner() {
    let mut invalid_cmd = Command::new("sleep");
    // provide invalid argument to sleep command
    invalid_cmd.arg("fdsa");

    let mut agent = BlockingSupervisor {
        cmd: ProcessRunner::new(invalid_cmd),
    };

    // run the process with wrong parameter
    assert_eq!(agent.cmd.run().unwrap().success(), false);

    let mut valid_cmd = Command::new("sleep");
    // provide invalid argument to sleep command
    valid_cmd.arg("1");

    agent.cmd = ProcessRunner::new(valid_cmd);

    // run the process with wrong parameter
    assert_eq!(agent.cmd.run().unwrap().success(), true);
}
