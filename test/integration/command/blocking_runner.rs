use newrelic_super_agent::command::{CommandRunner, ProcessRunner};
use std::collections::HashMap;

// blocking supervisor
struct BlockingSupervisor {
    agent_bin: String,
    agent_args: Vec<String>,
    agent_env: HashMap<String, String>,
}

impl From<&BlockingSupervisor> for ProcessRunner {
    fn from(value: &BlockingSupervisor) -> Self {
        ProcessRunner::new(&value.agent_bin, &value.agent_args, &value.agent_env)
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

    let mut proc: ProcessRunner = ProcessRunner::from(&agent);

    // run the process with wrong parameter
    assert!(!proc.run().unwrap().success());

    agent.agent_args = vec!["0.1".to_string()];

    proc = ProcessRunner::from(&agent);

    // run the process with correct parameter
    assert!(proc.run().unwrap().success());
}
