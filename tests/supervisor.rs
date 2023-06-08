use std::{sync::mpsc::Sender, thread, time::Duration};

use meta_agent::{
    command::stream::OutputEvent,
    supervisor::{context, runner::SupervisorRunner, Handle, Runner},
};

struct Config {
    tx: Sender<OutputEvent>,
}

impl From<&Config> for SupervisorRunner {
    fn from(value: &Config) -> Self {
        SupervisorRunner::new(
            "echo".to_string(),
            vec!["hello!".to_string()],
            context::SupervisorContext::new(),
            value.tx.clone(),
        )
    }
}

// How should this supervisor work?
#[test]
fn test_supervisors() {
    // Create streaming channel
    let (tx, rx) = std::sync::mpsc::channel();

    // Hypothetical meta agent configuration
    let conf = Config { tx };

    // Create 50 supervisors
    let agents: Vec<SupervisorRunner> = (0..50)
        .map(
            |_| {
                SupervisorRunner::from(&conf)
            }, /* TODO: I guess we could call `with_restart_policy()` here. */
        )
        .collect();

    // Run all the supervisors, getting handles
    let handles = agents
        .into_iter()
        .map(|agent| agent.run())
        .collect::<Vec<_>>();

    // Get any outputs
    thread::spawn(move || {
        rx.iter().for_each(|e| {
            println!("Received: {:?}", e);
        })
    });

    // Sleep for a while
    thread::sleep(Duration::from_secs(1));

    // Wait for all the supervised processes to finish
    let results = handles.into_iter().map(|h| h.stop().join());

    // Check that all the processes have finished correctly
    assert_eq!(results.flatten().count(), 50);
}
