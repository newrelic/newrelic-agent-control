use crate::on_host::tools::global_logger::init_logger;
use newrelic_super_agent::super_agent::config_patcher::ConfigPatcher;
use newrelic_super_agent::super_agent::config_storer::loader_storer::SuperAgentConfigLoader;
use newrelic_super_agent::super_agent::config_storer::store::SuperAgentConfigStore;
use newrelic_super_agent::super_agent::defaults::SUPER_AGENT_CONFIG_FILE;
use newrelic_super_agent::super_agent::run::{BasePaths, SuperAgentRunConfig, SuperAgentRunner};
use newrelic_super_agent::values::file::YAMLConfigRepositoryFile;
use std::sync::Arc;

/// Starts the super-agent and blocks while executing.
/// Take into account that some of the logic from main is not present here.
pub fn start_super_agent_with_custom_config(base_paths: BasePaths) {
    // logger is a global variable shared between all test threads
    init_logger();

    let super_agent_repository = YAMLConfigRepositoryFile::new(
        base_paths.super_agent_local_config.clone(),
        base_paths.remote_dir.join(SUPER_AGENT_CONFIG_FILE),
    );
    let config_storer = SuperAgentConfigStore::new(Arc::new(super_agent_repository));

    let mut super_agent_config = config_storer.load().unwrap();

    let config_patcher =
        ConfigPatcher::new(base_paths.local_dir.clone(), base_paths.log_dir.clone());
    config_patcher.patch(&mut super_agent_config);

    let run_config = SuperAgentRunConfig {
        opamp: super_agent_config.opamp,
        http_server: super_agent_config.server,
        base_paths,
    };

    SuperAgentRunner::try_from(run_config)
        .unwrap()
        .run()
        .unwrap();
}
