use std::env::args;
use std::thread;
use std::thread::sleep;
use std::time::Duration;

use serde_json::Value;

use meta_agent::{Agent, Resolver};

fn main() {
    let config_path = config_path_from_args();
    // Get agent's generic config to get agent's names
    let config_resolver = Resolver::from_path(config_path.as_str());
    let nextgen: Agent<Resolver, Value> = Agent::new(config_resolver);

    //TODO : <just for testing, remove>
    thread::spawn(move || {
        sleep(Duration::from_secs(10));
        println!("stopping the meta agent");
        nextgen.stop();
    });
    //TODO : </just for testing, remove>

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