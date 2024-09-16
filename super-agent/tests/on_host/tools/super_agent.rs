use crate::on_host::tools::global_logger::init_logger;
use newrelic_super_agent::event::channel::EventConsumer;
use newrelic_super_agent::event::ApplicationEvent;
use newrelic_super_agent::super_agent::config_storer::loader_storer::SuperAgentConfigLoader;
use newrelic_super_agent::super_agent::config_storer::store::SuperAgentConfigStore;
use newrelic_super_agent::super_agent::run::{BasePaths, SuperAgentRunConfig, SuperAgentRunner};
use newrelic_super_agent::values::file::YAMLConfigRepositoryFile;
use std::sync::Arc;

/// Starts the super-agent and blocks while executing.
/// Take into account that some of the logic from main is not present here.
pub fn start_super_agent_with_custom_config(
    base_paths: BasePaths,
    application_event_consumer: EventConsumer<ApplicationEvent>,
) {
    // logger is a global variable shared between all test threads
    init_logger();

    let super_agent_repository =
        YAMLConfigRepositoryFile::new(base_paths.local_dir.clone(), base_paths.remote_dir.clone());
    let config_storer = SuperAgentConfigStore::new(Arc::new(super_agent_repository));

    let super_agent_config = config_storer.load().unwrap();

    let run_config = SuperAgentRunConfig {
        opamp: super_agent_config.opamp,
        http_server: super_agent_config.server,
        base_paths,
    };

    // Create the actual super agent runner with the rest of required configs and the application_event_consumer
    SuperAgentRunner::new(run_config, application_event_consumer)
        .unwrap()
        .run()
        .unwrap();
}
