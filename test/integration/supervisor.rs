use std::{collections::HashMap, thread, time::Duration};

use newrelic_super_agent::{context::Context, logging::Logging};

use newrelic_super_agent::sub_agent::on_host::command::logger::{EventLogger, StdEventReceiver};
use newrelic_super_agent::sub_agent::on_host::supervisor::command_supervisor::NotStartedSupervisorOnHost;
use newrelic_super_agent::sub_agent::on_host::supervisor::command_supervisor_config::SupervisorConfigOnHost;
use newrelic_super_agent::sub_agent::on_host::supervisor::restart_policy::RestartPolicy;
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

    let conf = SupervisorConfigOnHost::new(
        "sh".to_string(),
        vec!["-c".to_string(), "sleep 2".to_string()],
        Context::new(),
        HashMap::default(),
        tx,
        RestartPolicy::default(),
    );

    // Create 50 supervisors
    let agents: Vec<NotStartedSupervisorOnHost> = (0..10)
        .map(
            |_| {
                NotStartedSupervisorOnHost::new(conf.clone())
            }, /* TODO: I guess we could call `with_restart_policy()` here. */
        )
        .collect();

    // Run all the supervisors, getting handles
    let handles = agents
        .into_iter()
        .map(|agent| agent.run().unwrap())
        .collect::<Vec<_>>();

    // Get any outputs in the background
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
