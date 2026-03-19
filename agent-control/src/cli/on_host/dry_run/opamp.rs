use std::sync::Arc;

use opamp_client::StartedClient;
use tracing::info;

use crate::{
    agent_control::{
        config_repository::repository::AgentControlConfigLoader,
        run::{
            on_host::{ac_identifiers, opamp_client_builder, start_ac_opamp_client},
            setup_config_repository_and_store,
        },
    },
    command::Context,
    on_host::file_store::FileStore,
    opamp::instance_id::{getter::InstanceIDWithIdentifiersGetter, storer::Storer},
};

pub fn check_connectivity(context: Context) -> Result<(), Box<dyn std::error::Error>> {
    let maybe_opamp = context.ac_runner_context.bootstrap_config.fleet_control;
    let Some(opamp) = maybe_opamp.as_ref() else {
        info!("OpAMP configuration not found. Skipping OpAMP connectivity check.");
        return Ok(());
    };

    let local_dir = context.ac_runner_context.base_paths.local_dir;
    let remote_dir = context.ac_runner_context.base_paths.remote_dir;
    let file_store = Arc::new(FileStore::new_local_fs(
        local_dir.clone(),
        remote_dir.clone(),
    ));

    let (yaml_config_repository, config_storer) =
        setup_config_repository_and_store(file_store.clone(), maybe_opamp.is_some());
    let agent_control_config = config_storer
        .load()
        .map_err(|err| format!("failed to load Agent Control config: {err}"))?;

    let identifiers = ac_identifiers(&agent_control_config)?;

    let instance_id_storer = Storer::from(file_store);
    let instance_id_getter =
        InstanceIDWithIdentifiersGetter::new(instance_id_storer, identifiers.clone());

    let proxy = context.ac_runner_context.bootstrap_config.proxy;
    let opamp_client_builder = opamp_client_builder(
        local_dir.clone(),
        opamp.clone(),
        proxy.clone(),
        yaml_config_repository.clone(),
    );

    // We are starting and immediately stopping the client just to check connectivity.
    // The client performs a connectivity check as part of its startup process.
    // However, we don't need the client to stay alive after the initial check, so we can stop it right away.
    //
    // This approach avoids having to implement a separate connectivity check logic, and it leverages the existing functionality of the OpAMP client.
    // It comes at a cost. A thread might be spawned between the start and stop, which we don't need. Besides, messages will be processed with
    // `process_message` (as part of the initial check), which is not needed.
    //
    // Long short story, the implementation leverages existing functionality at the cost of doing some unnecessary work.
    let (client, _consumer) =
        start_ac_opamp_client(&opamp_client_builder, &instance_id_getter, &identifiers)?;
    client.stop()?;

    info!("OpAMP connectivity check successful");

    Ok(())
}
