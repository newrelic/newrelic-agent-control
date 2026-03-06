use std::{collections::HashMap, sync::Arc};

use opamp_client::{StartedClient, operation::settings::DescriptionValueType};
use tracing::info;

use crate::{
    agent_control::{
        config_repository::repository::AgentControlConfigLoader,
        defaults::{AGENT_CONTROL_VERSION, OPAMP_AGENT_VERSION_ATTRIBUTE_KEY},
        run::{
            on_host::agent_control_opamp_non_identifying_attributes,
            setup_config_repository_and_store,
        },
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
};

pub fn check_connectivity(context: Context) -> Result<(), Box<dyn std::error::Error>> {
    let base_paths = context.ac_runner_context.base_paths;
    let local_dir = base_paths.local_dir;
    let remote_dir = base_paths.remote_dir;

    let file_store = Arc::new(FileStore::new_local_fs(
        local_dir.clone(),
        remote_dir.clone(),
    ));

    let maybe_opamp = context.ac_runner_context.bootstrap_config.fleet_control;

    let secret_retriever = OnHostSecretRetriever::new(
        maybe_opamp.clone(),
        local_dir.clone(),
        FileSecretProvider::new(),
    );

    let (yaml_config_repository, config_storer) =
        setup_config_repository_and_store(file_store.clone(), maybe_opamp.is_some());
    let agent_control_config = config_storer
        .load()
        .map_err(|err| format!("failed to load Agent Control config: {err}"))?;

    let fleet_id = agent_control_config
        .fleet_control
        .as_ref()
        .map(|c| c.fleet_id.to_string())
        .unwrap_or_default();

    let identifiers_provider = IdentifiersProvider::try_default()
        .map_err(|err| format!("failed to build the identifiers provider: {err}"))?
        .with_host_id(agent_control_config.host_id.to_string())
        .with_fleet_id(fleet_id);

    let identifiers = identifiers_provider
        .provide()
        .map_err(|err| format!("failure obtaining identifiers: {err}"))?;
    let non_identifying_attributes = agent_control_opamp_non_identifying_attributes(&identifiers);
    info!("Instance Identifiers: {:?}", identifiers);

    let instance_id_storer = Storer::from(file_store);
    let instance_id_getter = InstanceIDWithIdentifiersGetter::new(instance_id_storer, identifiers);

    let opamp_client_builder = maybe_opamp.map(|config| {
        OpAMPClientBuilder::new(
            config.poll_interval,
            OpAMPHttpClientBuilder::new(
                config,
                context.ac_runner_context.bootstrap_config.proxy.clone(),
                secret_retriever,
            ),
            EffectiveConfigLoaderBuilder::new(yaml_config_repository.clone()),
        )
    });

    // Build and start AC OpAMP client
    let (maybe_client, _maybe_sa_opamp_consumer) = opamp_client_builder
        .as_ref()
        .map(|builder| {
            build_opamp_with_channel(
                builder,
                &instance_id_getter,
                &AgentIdentity::new_agent_control_identity(),
                HashMap::from([(
                    OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
                    DescriptionValueType::String(AGENT_CONTROL_VERSION.to_string()),
                )]),
                non_identifying_attributes,
            )
        })
        // Transpose changes Option<Result<T, E>> to Result<Option<T>, E>, enabling the use of `?` to handle errors in this function
        .transpose()
        .map_err(|err| format!("error initializing OpAMP client: {err}"))?
        .map(|(client, consumer)| (Some(client), Some(consumer)))
        .unwrap_or_default();

    // We are starting and immediately stopping the client just to check connectivity.
    // The client performs a connectivity check as part of its startup process.
    // However, we don't need the client to stay alive after the initial check, so we can stop it right away.
    //
    // This approach avoids having to implement a separate connectivity check logic, and it leverages the existing functionality of the OpAMP client.
    // It comes at a cost. A thread might be spawned between the start and stop, which we don't need. Besides, messages will be processed with
    // `process_message` (as part of the initial check), which is not needed.
    //
    // Long short story, the implementation leverages existing functionality at the cost of doing some unnecessary work.
    maybe_client.map(|c| c.stop());

    info!("OpAMP connectivity check successful");

    Ok(())
}
