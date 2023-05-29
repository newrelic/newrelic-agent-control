use std::env::args;

use serde_json::Value;

use meta_agent::{Agent, Resolver};

fn main() {
    let config_path = config_path_from_args();
    // Get agent's generic config to get agent's names
    let config_resolver = Resolver::from_path(config_path.as_str());
    let nextgen: Agent<Resolver, Value> = Agent::new(config_resolver);
    if let Err(err) = nextgen.start() {
        eprintln!("{}", err);
        std::process::exit(1);
    }
}

fn config_path_from_args() -> String {
    let args: Vec<String> = args().collect();
    if args.len() > 1 {
        args.get(1).unwrap().to_string()
    } else {
        String::from("config/nr_meta_agent.yaml")
    }
}