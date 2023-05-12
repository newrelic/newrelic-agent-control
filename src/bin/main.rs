use std::env::args;
use std::thread;

use serde_json::Value;

use meta_agent::{Agent, Config, InfraAgentSupervisor, NrDotSupervisor, Resolver};
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

    //https://stackoverflow.com/questions/55490906/why-does-spawning-threads-using-iteratormap-not-run-the-threads-in-parallel
    // let supervisors: Vec<Box<dyn Supervisor>> = vec![Box::new(InfraAgentSupervisor::new()), Box::new(NrDotSupervisor::new())];
    // supervisors.iter().map(|spv| {
    //     thread::spawn(|| {
    //         let mut infra_supervisor = InfraAgentSupervisor::new();
    //         match infra_supervisor.start() {
    //             Err(e) => println!("{}", e),
    //             Ok(()) => println!("all good pavo!")
    //         }
    //     })
    // }).for_each(|handle| { handle.join().unwrap() });

    let handle1 = thread::spawn(|| {
        let mut infra_supervisor = InfraAgentSupervisor::new();
        match infra_supervisor.start() {
            Err(e) => println!("{}", e),
            Ok(()) => println!("all good pavo!")
        }
    });

    let handle2 = thread::spawn(|| {
        let mut nrdot_supervisor = NrDotSupervisor::new();
        match nrdot_supervisor.start() {
            Err(e) => println!("{}", e),
            Ok(()) => println!("all good pavo!")
        }
    });

    handle1.join().unwrap();
    handle2.join().unwrap();
}
