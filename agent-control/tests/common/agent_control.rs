use crate::common::global_logger::init_logger;
use newrelic_agent_control::agent_control::config::K8sConfig;
use newrelic_agent_control::agent_control::config_repository::repository::AgentControlConfigLoader;
use newrelic_agent_control::agent_control::config_repository::store::AgentControlConfigStore;
use newrelic_agent_control::agent_control::run::{
    AgentControlRunConfig, AgentControlRunner, BasePaths, Environment,
};
use newrelic_agent_control::event::ApplicationEvent;
use newrelic_agent_control::event::channel::{EventPublisher, pub_sub};
use newrelic_agent_control::http::tls::install_rustls_default_crypto_provider;
use newrelic_agent_control::values::file::ConfigRepositoryFile;
use std::sync::Arc;
use std::time::Duration;

/// Starts the agent-control in a separate thread. The agent-control will be stopped when the `StartedAgentControl` is dropped.
/// Take into account that some of the logic from main is not present here.
pub fn start_agent_control_with_custom_config(
    base_paths: BasePaths,
    mode: Environment,
) -> StartedAgentControl {
    install_rustls_default_crypto_provider();

    let (application_event_publisher, application_event_consumer) = pub_sub();

    let handle = std::thread::spawn(move || {
        // logger is a global variable shared between all test threads
        init_logger();

        let agent_control_repository =
            ConfigRepositoryFile::new(base_paths.local_dir.clone(), base_paths.remote_dir.clone());
        let config_storer = AgentControlConfigStore::new(Arc::new(agent_control_repository));

        let agent_control_config = config_storer.load().unwrap();

        // TODO - Temporal solution until https://new-relic.atlassian.net/browse/NR-343594 is done.
        // There is a current issue with the diff computation the GC does in order to collect agents. If a new agent is added and removed
        // before the GC process it, the resources will never be collected.
        let opamp_poll_interval = Duration::from_secs(5);

        let run_config = AgentControlRunConfig {
            opamp: agent_control_config.fleet_control,
            opamp_poll_interval,
            http_server: agent_control_config.server,
            base_paths,
            proxy: agent_control_config.proxy,

            k8s_config: match mode {
                // This config is not used on the OnHost environment, a blank config is used.
                // K8sConfig has not default since cluster_name is a required.
                Environment::OnHost => K8sConfig {
                    cluster_name: Default::default(),
                    client_config: Default::default(),
                    namespace: Default::default(),
                    namespace_agents: Default::default(),
                    chart_version: Default::default(),
                    cr_type_meta: Default::default(),
                },
                Environment::K8s => {
                    let mut cfg = agent_control_config
                        .k8s
                        .expect("K8s config must be present when running in K8s");

                    cfg.client_config.client_timeout = Duration::from_secs(30).into();
                    cfg
                }
            },
        };

        // Create the actual agent control runner with the rest of required configs and the application_event_consumer
        AgentControlRunner::new(run_config, application_event_consumer)
            .unwrap()
            .run(mode)
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
