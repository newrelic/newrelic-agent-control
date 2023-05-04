use crate::agent::Agent;
use crate::config::resolver::Resolver;

mod agent;
mod config;

fn main() {
    let config_resolver = Resolver::new();
    let nextgen:Agent<Resolver> = Agent::new(config_resolver);
    if let Err(err) = nextgen.start() {
        eprintln!("{}", err);
        std::process::exit(1);
    }
}

