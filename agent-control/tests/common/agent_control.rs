use crate::common::global_logger::init_logger;
use crate::on_host::tools::config::create_file;
use newrelic_agent_control::agent_control::config_repository::repository::AgentControlConfigLoader;
use newrelic_agent_control::agent_control::config_repository::store::AgentControlConfigStore;
use newrelic_agent_control::agent_control::defaults::AUTH_PRIVATE_KEY_FILE_NAME;
use newrelic_agent_control::agent_control::run::{AgentControlRunner, BasePaths, Environment};
use newrelic_agent_control::event::ApplicationEvent;
use newrelic_agent_control::event::channel::{EventPublisher, pub_sub};
use newrelic_agent_control::on_host::file_store::FileStore;
use newrelic_agent_control::values::ConfigRepo;
use std::sync::Arc;

/// Starts the agent-control in a separate thread. The agent-control will be stopped when the `StartedAgentControl` is dropped.
/// Take into account that some of the logic from main is not present here.
pub fn start_agent_control_with_custom_config(
    base_paths: BasePaths,
    ac_running_mode: Environment,
) -> StartedAgentControl {
    let (application_event_publisher, application_event_consumer) = pub_sub();

    let handle = std::thread::spawn(move || {
        // logger is a global variable shared between all test threads
        init_logger();

        if ac_running_mode != Environment::K8s {
            create_file(
                "dummy-private-key-content".to_string(),
                base_paths.local_dir.join(AUTH_PRIVATE_KEY_FILE_NAME),
            );
        }

        let file_store = Arc::new(FileStore::new_local_fs(
            base_paths.local_dir.clone(),
            base_paths.remote_dir.clone(),
        ));
        let agent_control_repository = ConfigRepo::new(file_store);
        let config_storer = AgentControlConfigStore::new(Arc::new(agent_control_repository));

        let agent_control_config = config_storer.load().unwrap();

        // Create the actual agent control runner with the rest of required configs and the application_event_consumer
        AgentControlRunner::try_new(
            agent_control_config,
            base_paths,
            ac_running_mode,
            application_event_consumer,
        )
        .unwrap()
        .run()
        .unwrap();
    });

    StartedAgentControl {
        application_event_publisher,
        handle: Some(handle),
    }
}

pub struct StartedAgentControl {
    application_event_publisher: EventPublisher<ApplicationEvent>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for StartedAgentControl {
    fn drop(&mut self) {
        self.application_event_publisher
            .publish(ApplicationEvent::StopRequested)
            .unwrap();

        self.handle
            .take()
            .expect("handle should exist")
            .join()
            .expect("joining handle");
    }
}
