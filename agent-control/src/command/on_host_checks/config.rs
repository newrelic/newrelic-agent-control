use std::{path::PathBuf, sync::Arc};

use fs::{directory_manager::DirectoryManagerFs, file::LocalFile};
use tracing::info;

use crate::{
    agent_control::{
        builder::{Environment, setup_config_repository_and_store},
        config::{AgentControlConfig, OpAMPClientConfig},
        config_repository::repository::AgentControlConfigLoader,
    },
    command::{Args, Command},
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

pub fn check_config(
    running_mode: Environment,
    args: &Args,
) -> Result<VerifiedConfig, Box<dyn std::error::Error>> {
    let context = Command::build_context(
        running_mode,
        args,
        #[cfg(target_os = "windows")]
        false,
    )
    .map_err(|err| format!("failed to build context: {err}"))?;

    let local_dir = context.ac_runner_context.base_paths.local_dir;
    let remote_dir = context.ac_runner_context.base_paths.remote_dir;
    let file_store = Arc::new(FileStore::new_local_fs(
        local_dir.clone(),
        remote_dir.clone(),
    ));

    let maybe_opamp = context.ac_runner_context.bootstrap_config.fleet_control;
    let (yaml_config_repository, config_storer) =
        setup_config_repository_and_store(file_store.clone(), maybe_opamp.is_some());
    let agent_control_config = config_storer
        .load()
        .map_err(|err| format!("failed to load Agent Control config: {err}"))?;

    info!("Config validation check successful");

    let proxy_config = context.ac_runner_context.bootstrap_config.proxy.clone();

    Ok(VerifiedConfig {
        local_dir,
        maybe_opamp,
        proxy_config,
        agent_control_config,
        file_store,
        yaml_config_repository,
    })
}
