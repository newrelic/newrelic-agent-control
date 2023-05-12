use std::env::args;

use serde_json::Value;

use meta_agent::{Agent, Config, Resolver};

fn main() {
    let args: Vec<String> = args().collect();
    let path: &str = if args.len() > 1 {
        args.get(1).unwrap()
    } else {
        "config/nr_meta_agent.json"
    };

    let config_resolver = Resolver::from_path(path);
    let nextgen: Agent<Config<Value>, Resolver, Value> = Agent::new(config_resolver);
    if let Err(err) = nextgen.start() {
        eprintln!("{}", err);
        std::process::exit(1);
    }
}
