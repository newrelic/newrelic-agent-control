use std::{thread, time::Duration};

use newrelic_super_agent::context::Context;
use newrelic_super_agent::logging::config::LoggingConfig;

use newrelic_super_agent::event::channel::pub_sub;
use newrelic_super_agent::sub_agent::on_host::supervisor::command_supervisor::{
    NotStarted, SupervisorOnHost,
};
use newrelic_super_agent::sub_agent::on_host::supervisor::command_supervisor_config::SupervisorConfigOnHost;
use newrelic_super_agent::sub_agent::restart_policy::RestartPolicy;
use std::sync::Once;

static INIT_LOGGER: Once = Once::new();

pub fn init_logger() {
    INIT_LOGGER.call_once(|| {
        LoggingConfig::default().try_init().unwrap();
    });
}

// only unix: shutdown is not implemented for Windows
#[cfg(target_family = "unix")]
#[test]
fn test_supervisors() {
    use std::thread::JoinHandle;

    use newrelic_super_agent::sub_agent::on_host::supervisor::command_supervisor_config::ExecutableData;

    init_logger();

    let agent_id = "sleep-test".to_string().try_into().unwrap();

    let exec = ExecutableData::new("sh".to_string())
        .with_args(vec!["-c".to_string(), "sleep 2".to_string()]);

    let conf =
        SupervisorConfigOnHost::new(agent_id, exec, Context::new(), RestartPolicy::default());

    // Create 50 supervisors
    let agents: Vec<SupervisorOnHost<NotStarted>> = (0..10)
        .map(
            |_| {
                SupervisorOnHost::new(conf.clone())
            }, /* TODO: I guess we could call `with_restart_policy()` here. */
        )
        .collect();

    let (sub_agent_internal_publisher, _sub_agent_internal_consumer) = pub_sub();

    // Run all the supervisors, getting handles
    let handles = agents
        .into_iter()
        .map(|agent| agent.run(sub_agent_internal_publisher.clone()))
        .collect::<Vec<_>>();

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
}
