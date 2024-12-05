use crate::common::global_logger::init_logger;
use newrelic_super_agent::event::channel::{pub_sub, EventPublisher};
use newrelic_super_agent::event::ApplicationEvent;
use newrelic_super_agent::http::tls::install_rustls_default_crypto_provider;
use newrelic_super_agent::super_agent::config_storer::loader_storer::SuperAgentConfigLoader;
use newrelic_super_agent::super_agent::config_storer::store::SuperAgentConfigStore;
use newrelic_super_agent::super_agent::run::{BasePaths, SuperAgentRunConfig, SuperAgentRunner};
use newrelic_super_agent::values::file::YAMLConfigRepositoryFile;
use std::sync::Arc;
use std::time::Duration;

/// Starts the super-agent in a separate thread. The super-agent will be stopped when the `StartedSuperAgent` is dropped.
/// Take into account that some of the logic from main is not present here.
pub fn start_super_agent_with_custom_config(base_paths: BasePaths) -> StartedSuperAgent {
    install_rustls_default_crypto_provider();

    let (application_event_publisher, application_event_consumer) = pub_sub();

    let handle = std::thread::spawn(move || {
        // logger is a global variable shared between all test threads
        init_logger();

        let super_agent_repository = YAMLConfigRepositoryFile::new(
            base_paths.local_dir.clone(),
            base_paths.remote_dir.clone(),
        );
        let config_storer = SuperAgentConfigStore::new(Arc::new(super_agent_repository));

        let super_agent_config = config_storer.load().unwrap();

        let opamp_poll_interval = Duration::from_secs(2);
        let garbage_collector_interval = Duration::from_secs(1);
        // TODO - Temporal solution until https://new-relic.atlassian.net/browse/NR-343594 is done.
        // There is a current issue with the diff computation the GC does in order to collect agents. If a new agent is added and removed
        // before the GC process it, the resources will never be collected.
        assert!(opamp_poll_interval > garbage_collector_interval);

        let run_config = SuperAgentRunConfig {
            opamp: super_agent_config.opamp,
            opamp_poll_interval,
            http_server: super_agent_config.server,
            base_paths,
            proxy: super_agent_config.proxy,
            #[cfg(feature = "k8s")]
            k8s_config: super_agent_config.k8s.unwrap(),
            #[cfg(feature = "k8s")]
            garbage_collector_interval,
        };

        // Create the actual super agent runner with the rest of required configs and the application_event_consumer
        SuperAgentRunner::new(run_config, application_event_consumer)
            .unwrap()
            .run()
            .unwrap();
    });

    StartedSuperAgent {
        application_event_publisher,
        handle: Some(handle),
    }
}

pub struct StartedSuperAgent {
    application_event_publisher: EventPublisher<ApplicationEvent>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for StartedSuperAgent {
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
