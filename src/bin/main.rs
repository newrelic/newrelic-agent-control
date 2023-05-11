use meta_agent::{Agent, Resolver};
use serde_json::Value;

fn main() {
    let config_resolver = Resolver::new();
    let nextgen: Agent<Resolver, Value> = Agent::new(config_resolver);
    if let Err(err) = nextgen.start() {
        eprintln!("{}", err);
        std::process::exit(1);
    }
}
