use std::{collections::HashMap, sync::mpsc::Sender, thread, time::Duration};

use newrelic_super_agent::{
    command::{stream::Event, EventLogger, StdEventReceiver},
    context::Context,
    logging::Logging,
    supervisor::{runner::SupervisorRunner, Handle, Runner},
};

struct Config {
    tx: Sender<Event>,
}

impl From<&Config> for SupervisorRunner {
    fn from(value: &Config) -> Self {
        SupervisorRunner::new(
            "sh".to_string(),
            vec!["-c".to_string(), "sleep 2".to_string()],
            Context::new(),
            HashMap::default(),
            value.tx.clone(),
        )
    }
}

use std::sync::Once;

static INIT_LOGGER: Once = Once::new();
pub fn init_logger() {
    INIT_LOGGER.call_once(|| {
        Logging::try_init().unwrap();
    });
}

// only unix: shutdown is not implemented for Windows
#[cfg(target_family = "unix")]
#[test]
fn test_supervisors() {
    use std::thread::JoinHandle;

    init_logger();

    // Create streaming channel
    let (tx, rx) = std::sync::mpsc::channel();

    let logger = StdEventReceiver::default();

    // Hypothetical super agent configuration
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
    let results: Vec<JoinHandle<()>> = handles.into_iter().map(|h| h.stop()).collect();

    // Check that all the processes have finished correctly
    assert_eq!(
        results.into_iter().flat_map(|handle| handle.join()).count(),
        10
    );

    drop(conf);
    // ensure logger was terminated
    handle_logger.join().unwrap();
}
