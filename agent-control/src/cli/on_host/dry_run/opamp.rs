use std::{collections::HashMap, sync::Arc};

use opamp_client::{StartedClient, operation::settings::DescriptionValueType};
use resource_detection::cloud::http_client::DEFAULT_CLIENT_TIMEOUT;
use tracing::{debug, info};

use crate::{
    agent_control::{
        config_repository::{repository::AgentControlConfigLoader, store::AgentControlConfigStore},
        defaults::{
            AGENT_CONTROL_VERSION, ENVIRONMENT_VARIABLES_FILE_NAME,
            OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
        },
        run::{
            BasePaths, on_host::agent_control_opamp_non_identifying_attributes,
            opamp_client_builder,
        },
    },
    command::InitError,
    http::{
        client::HttpClient,
        config::{HttpConfig, ProxyConfig},
    },
    on_host::file_store::FileStore,
    opamp::{
        instance_id::{
            getter::InstanceIDWithIdentifiersGetter, on_host::identifiers::IdentifiersProvider,
            storer::Storer,
        },
        operations::build_opamp_with_channel,
    },
    secret_retriever::on_host::retrieve::OnHostSecretRetriever,
    secrets_provider::file::FileSecretProvider,
    sub_agent::identity::AgentIdentity,
    utils::env_var::load_env_yaml_file,
    values::ConfigRepo,
};

pub fn check_connectivity() -> Result<(), Box<dyn std::error::Error>> {
    let base_paths = BasePaths::default();
    let local_dir = base_paths.local_dir;
    let remote_dir = base_paths.remote_dir;

    let env_file_path = local_dir.join(ENVIRONMENT_VARIABLES_FILE_NAME);
    if env_file_path.exists() {
        let path = env_file_path.display();
        info!("Loading environment variables from: {}", path);

        load_env_yaml_file(env_file_path.as_path())
            .map_err(|err| format!("failed to load environment variables from {path}: {err}"))?;
    };

    let file_store = Arc::new(FileStore::new_local_fs(
        local_dir.clone(),
        remote_dir.clone(),
    ));

    let agent_control_config_repository = ConfigRepo::new(file_store.clone());
    let agent_control_config =
        AgentControlConfigStore::new(Arc::new(agent_control_config_repository))
            .load()
            .map_err(|err| {
                InitError::LoaderError(local_dir.to_string_lossy().to_string(), err.to_string())
            })?;

    let proxy = agent_control_config
        .proxy
        .try_with_url_from_env()
        .map_err(|err| InitError::InvalidConfig(err.to_string()))?;

    let opamp = agent_control_config.fleet_control;
    if opamp.is_none() {
        info!("Fleet Control configuration not found, skipping OpAMP connectivity check");
        return Ok(());
    }

    let secret_retriever =
        OnHostSecretRetriever::new(opamp.clone(), local_dir.clone(), FileSecretProvider::new());

    debug!("Initializing yaml_config_repository");
    let config_repository = ConfigRepo::new(file_store.clone());
    let yaml_config_repository = Arc::new(if opamp.is_some() {
        config_repository.with_remote()
    } else {
        config_repository
    });

    let config_storer = Arc::new(AgentControlConfigStore::new(yaml_config_repository.clone()));
    let agent_control_config = config_storer
        .load()
        .map_err(|err| format!("failed to load Agent Control config: {err}"))?;

    let fleet_id = agent_control_config
        .fleet_control
        .as_ref()
        .map(|c| c.fleet_id.to_string())
        .unwrap_or_default();

    // The proxy is not required for the identifiers.
    // Cloud providers and internal endpoints should be reachable without the proxy.
    let identifiers_http_client = HttpClient::new(HttpConfig::new(
        DEFAULT_CLIENT_TIMEOUT,
        DEFAULT_CLIENT_TIMEOUT,
        ProxyConfig::default(),
    ))
    .map_err(|err| format!("failed to create http client: {err}"))?;

    let identifiers_provider = IdentifiersProvider::new(identifiers_http_client)
        .with_host_id(agent_control_config.host_id.to_string())
        .with_fleet_id(fleet_id);

    let identifiers = identifiers_provider
        .provide()
        .map_err(|err| format!("failure obtaining identifiers: {err}"))?;
    let non_identifying_attributes = agent_control_opamp_non_identifying_attributes(&identifiers);
    info!("Instance Identifiers: {:?}", identifiers);

    let instance_id_storer = Storer::from(file_store);
    let instance_id_getter = InstanceIDWithIdentifiersGetter::new(instance_id_storer, identifiers);

    let opamp_client_builder = opamp
        .map(|config| {
            opamp_client_builder(
                config,
                proxy.clone(),
                secret_retriever,
                yaml_config_repository.clone(),
            )
        })
        .transpose()?;

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
