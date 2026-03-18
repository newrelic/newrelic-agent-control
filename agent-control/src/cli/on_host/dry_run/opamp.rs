use std::{collections::HashMap, sync::Arc};

use opamp_client::{StartedClient, operation::settings::DescriptionValueType};
use tracing::info;

use crate::{
    agent_control::{
        defaults::{AGENT_CONTROL_VERSION, OPAMP_AGENT_VERSION_ATTRIBUTE_KEY},
        run::on_host::agent_control_opamp_non_identifying_attributes,
    },
    command::Context,
    on_host::file_store::FileStore,
    opamp::{
        client_builder::OpAMPClientBuilder,
        effective_config::loader::EffectiveConfigLoaderBuilder,
        http::builder::OpAMPHttpClientBuilder,
        instance_id::{
            getter::InstanceIDWithIdentifiersGetter, on_host::identifiers::IdentifiersProvider,
            storer::Storer,
        },
        operations::build_opamp_with_channel,
    },
    secret_retriever::on_host::retrieve::OnHostSecretRetriever,
    secrets_provider::file::FileSecretProvider,
    sub_agent::identity::AgentIdentity,
    values::ConfigRepo,
};

pub fn check_connectivity(context: Context) -> Result<(), Box<dyn std::error::Error>> {
    let maybe_opamp = context.ac_runner_context.bootstrap_config.fleet_control;
    let Some(opamp) = maybe_opamp else {
        info!("OpAMP configuration not found. Skipping OpAMP connectivity check.");
        return Ok(());
    };

    let fleet_id = opamp.fleet_id.clone();
    let poll_interval = opamp.poll_interval;

    let base_paths = context.ac_runner_context.base_paths;
    let secret_retriever = OnHostSecretRetriever::new(
        Some(opamp.clone()),
        base_paths.local_dir.clone(),
        FileSecretProvider::new(),
    );
    let http_client_builder = OpAMPHttpClientBuilder::new(
        opamp,
        context.ac_runner_context.bootstrap_config.proxy.clone(),
        secret_retriever,
    );

    let file_store = Arc::new(FileStore::new_local_fs(
        base_paths.local_dir.clone(),
        base_paths.remote_dir.clone(),
    ));
    let loader = EffectiveConfigLoaderBuilder::new(Arc::new(ConfigRepo::new(file_store.clone())));

    let opamp_client_builder = OpAMPClientBuilder::new(poll_interval, http_client_builder, loader);

    let identifiers_provider = IdentifiersProvider::try_default()
        .map_err(|err| format!("failed to build the identifiers provider: {err}"))?
        .with_host_id(
            context
                .ac_runner_context
                .bootstrap_config
                .host_id
                .to_string(),
        )
        .with_fleet_id(fleet_id.to_string());
    let identifiers = identifiers_provider
        .provide()
        .map_err(|err| format!("failure obtaining identifiers: {err}"))?;
    let non_identifying_attributes = agent_control_opamp_non_identifying_attributes(&identifiers);
    info!("Instance Identifiers: {:?}", identifiers);
    let instance_id_storer = Storer::from(file_store.clone());
    let instance_id_getter = InstanceIDWithIdentifiersGetter::new(instance_id_storer, identifiers);

    // Build and start AC OpAMP client
    let (client, _consumer) = build_opamp_with_channel(
        &opamp_client_builder,
        &instance_id_getter,
        &AgentIdentity::new_agent_control_identity(),
        HashMap::from([(
            OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
            DescriptionValueType::String(AGENT_CONTROL_VERSION.to_string()),
        )]),
        non_identifying_attributes,
    )
    .map_err(|err| format!("error initializing OpAMP client: {err}"))?;

    // We are starting and immediately stopping the client just to check connectivity.
    // The client performs a connectivity check as part of its startup process.
    // However, we don't need the client to stay alive after the initial check, so we can stop it right away.
    //
    // This approach avoids having to implement a separate connectivity check logic, and it leverages the existing functionality of the OpAMP client.
    // It comes at a cost. A thread might be spawned between the start and stop, which we don't need. Besides, messages will be processed with
    // `process_message` (as part of the initial check), which is not needed.
    //
    // Long short story, the implementation leverages existing functionality at the cost of doing some unnecessary work.
    client.stop()?;

    info!("OpAMP connectivity check successful");

    Ok(())
}
