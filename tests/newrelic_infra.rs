use std::{thread, time::Duration};

use log::info;
use meta_agent::supervisor::{newrelic_infra::NRIConfig, runner::SupervisorRunner, Handle, Runner};
use test_log;

// Using NewRelicInfra supervisor
#[test_log::test]
fn newrelic_infra_supervisor() {
    // Create streaming channel
    let (tx, rx) = std::sync::mpsc::channel();

    // Hypothetical meta agent configuration for NewRelicInfra
    let conf = NRIConfig { tx };

    // Create a newrelic-infra supervisor instance
    let agent = SupervisorRunner::from(&conf);

    // Run the supervisor, getting a handle
    let handle = agent.run();

    // Get agent outputs
    thread::spawn(move || {
        rx.iter().for_each(|e| {
            info!(target:"newrelic-infra supervisor", "output event: {:?}", e);
        })
    });

    // Sleep for a while
    thread::sleep(Duration::from_secs(15));

    // Stop the supervised process
    let result = handle.stop().join();

    // Check that the process has finished correctly
    assert!(result.is_ok());
}
