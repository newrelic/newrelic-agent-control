use std::env::args;
use std::thread;
use std::thread::JoinHandle;

use serde_json::Value;

use meta_agent::{Agent, Config, Resolver};
use meta_agent::supervisor::factory::SupervisorFactory;

fn main() {
    let args: Vec<String> = args().collect();
    let path: &str = if args.len() > 1 {
        args.get(1).unwrap()
    } else {
        "config/nr_meta_agent.json"
    };


    // Get agent's generic config to get agent's names
    let config_resolver = Resolver::from_path(path);
    let nextgen: Agent<Config<Value>, Resolver, Config<Value>> = Agent::new(config_resolver);
    let conf = nextgen.conf();
    if conf.is_err() {
        eprintln!("{}", conf.err().unwrap());
        std::process::exit(1);
    }


    //supervisors handles to wait until finish
    let mut handles: Vec<JoinHandle<()>> = Vec::new();

    for (agent_name, conf) in conf.unwrap().agents() {
        // serialize agent config as poc to deal with different configs per supervisor
        let serialized_conf = serde_json::to_string(conf);
        if serialized_conf.is_err() {
            eprintln!("{}", serialized_conf.err().unwrap());
            continue;
        }

        // build the supervisor based on the name and the supervisor raw config
        let supervisor = SupervisorFactory::from_config(agent_name.clone(), serialized_conf.unwrap());
        if supervisor.is_err() {
            eprintln!("{}", supervisor.err().unwrap());
            continue;
        }

        // run all supervisors
        let handle = thread::spawn(move || {
            match supervisor.unwrap().start() {
                Err(e) => println!("{}", e),
                Ok(()) => println!("all good pavo!")
            }
        });

        handles.push(handle);
    }

    for h in handles {
        h.join().unwrap();
    }
}
