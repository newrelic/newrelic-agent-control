use std::env::args;

use serde_json::Value;

use meta_agent::{Agent, Config, InfraAgentSupervisor, Resolver};
use meta_agent::supervisor::supervisor::Supervisor;

fn main() {
    let args: Vec<String> = args().collect();
    let path: &str = if args.len() > 1 {
        args.get(1).unwrap()
    } else {
        "config/nr_meta_agent.json"
    };

    // testing some conf unmarshalling
    let config_resolver = Resolver::from_path(path);
    let nextgen: Agent<Config<Value>, Resolver, Value> = Agent::new(config_resolver);
    if let Err(err) = nextgen.start() {
        eprintln!("{}", err);
        std::process::exit(1);
    }

    let mut infra_supervisor = InfraAgentSupervisor::new();
    match infra_supervisor.start() {
        Err(e) => println!("{}", e),
        Ok(()) => println!("all good pavo!")
    }
}
