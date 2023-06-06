use std::{sync::mpsc::Sender, thread, time::Duration};

use meta_agent::{
    agent::logging,
    command::{stream::Event, EventLogger, StdEventReceiver},
    supervisor::{backoff, context, runner::SupervisorRunner, Handle, Runner},
};
use meta_agent::supervisor::backoff::Backoff;

struct Config {
    tx: Sender<Event>,
}

impl From<&Config> for SupervisorRunner {
    fn from(value: &Config) -> Self {
        SupervisorRunner::new(
            "echo".to_string(),
            vec!["hello!".to_string()],
            context::SupervisorContext::new(),
            value.tx.clone(),
            backoff::BackoffStrategy::None,
        )
    }
}

use std::sync::Once;

static INIT_LOGGER: Once = Once::new();
pub fn init_logger() {
    INIT_LOGGER.call_once(|| {
        logging::init().unwrap();
    });
}

// How should this supervisor work?
#[test]
fn test_supervisors() {
    init_logger();

    // Create streaming channel
    let (tx, rx) = std::sync::mpsc::channel();

    let logger = StdEventReceiver::default();

    // Hypothetical meta agent configuration
    let conf = Config { tx };

    // Create 50 supervisors
    let agents: Vec<SupervisorRunner> = (0..10)
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

    // Get any outputs in the background
    //
    let handle_logger = logger.log(rx);

    // Sleep for a while
    thread::sleep(Duration::from_secs(1));

    // Wait for all the supervised processes to finish
    let results = handles.into_iter().map(|h| h.stop().join());

    // Check that all the processes have finished correctly
    assert_eq!(results.flatten().count(), 10);

    drop(conf);
    // ensure logger was terminated
    handle_logger.join().unwrap();
}
