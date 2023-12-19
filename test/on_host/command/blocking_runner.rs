use newrelic_super_agent::sub_agent::on_host::command::command::SyncCommandRunner;
use newrelic_super_agent::sub_agent::on_host::command::command_os::{CommandOS, Sync};
use std::collections::HashMap;

// blocking supervisor
struct BlockingSupervisor {
    agent_bin: String,
    agent_args: Vec<String>,
    agent_env: HashMap<String, String>,
}

impl From<&BlockingSupervisor> for CommandOS<Sync> {
    fn from(value: &BlockingSupervisor) -> Self {
        CommandOS::<Sync>::new(&value.agent_bin, &value.agent_args, &value.agent_env)
    }
}

#[test]
fn blocking_stop_runner() {
    let mut agent = BlockingSupervisor {
        // provide invalid argument to sleep command
        agent_bin: "sleep".to_string(),
        agent_args: vec!["fdsa".to_string()],
        agent_env: HashMap::default(),
    };

    let mut command = CommandOS::from(&agent);

    // run the process with wrong parameter
    assert!(!command.run().unwrap().success());

    agent.agent_args = vec!["0.1".to_string()];

    command = CommandOS::from(&agent);

    // run the process with correct parameter
    assert!(command.run().unwrap().success());
}
