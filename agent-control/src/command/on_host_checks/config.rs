use std::{path::PathBuf, sync::Arc};

use fs::{directory_manager::DirectoryManagerFs, file::LocalFile};
use tracing::info;

use crate::{
    agent_control::{
        config::{AgentControlConfig, OpAMPClientConfig},
        config_repository::repository::AgentControlConfigLoader,
        run::setup_config_repository_and_store,
    },
    command::{Args, BootstrapContext, Command},
    http::config::ProxyConfig,
    on_host::file_store::FileStore,
    values::ConfigRepo,
};

pub struct VerifiedConfig {
    pub local_dir: PathBuf,
    pub maybe_opamp: Option<OpAMPClientConfig>,
    pub proxy_config: ProxyConfig,
    pub agent_control_config: AgentControlConfig,
    pub file_store: Arc<FileStore<LocalFile, DirectoryManagerFs>>,
    pub yaml_config_repository: Arc<ConfigRepo<FileStore<LocalFile, DirectoryManagerFs>>>,
}

pub fn check_config(args: &Args) -> Result<VerifiedConfig, Box<dyn std::error::Error>> {
    let BootstrapContext {
        base_paths,
        bootstrap_config,
    } = Command::build_bootstrap_context(args)
        .map_err(|err| format!("failed to build context: {err}"))?;

    let local_dir = base_paths.local_dir;
    let remote_dir = base_paths.remote_dir;
    let file_store = Arc::new(FileStore::new_local_fs(
        local_dir.clone(),
        remote_dir.clone(),
    ));

    let maybe_opamp = bootstrap_config.fleet_control;
    let (yaml_config_repository, config_storer) =
        setup_config_repository_and_store(file_store.clone(), maybe_opamp.is_some());
    let agent_control_config = config_storer
        .load()
        .map_err(|err| format!("failed to load Agent Control config: {err}"))?;

    info!("Config validation check successful");

    let proxy_config = bootstrap_config.proxy.clone();

    Ok(VerifiedConfig {
        local_dir,
        maybe_opamp,
        proxy_config,
        agent_control_config,
        file_store,
        yaml_config_repository,
    })
}
