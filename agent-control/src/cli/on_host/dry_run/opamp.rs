use std::sync::Arc;

use opamp_client::{http::http_client::HttpClient as OpampHttpClient, opamp::proto::AgentToServer};
use prost::Message;
use resource_detection::cloud::http_client::DEFAULT_CLIENT_TIMEOUT;
use tracing::{debug, info};

use crate::{
    agent_control::{
        agent_id::AgentID,
        config_repository::{repository::AgentControlConfigLoader, store::AgentControlConfigStore},
        run::RunError,
    },
    command::RunContext,
    http::{
        client::HttpClient,
        config::{HttpConfig, ProxyConfig},
    },
    on_host::file_store::FileStore,
    opamp::{
        http::builder::HttpClientBuilder,
        instance_id::{
            getter::{InstanceIDGetter, InstanceIDWithIdentifiersGetter},
            on_host::identifiers::IdentifiersProvider,
            storer::Storer,
        },
        operations::build_opamp_http_builder,
    },
    secret_retriever::on_host::retrieve::OnHostSecretRetriever,
    secrets_provider::file::FileSecretProvider,
    values::ConfigRepo,
};

pub fn check_connectivity(run_context: RunContext) -> Result<(), Box<dyn std::error::Error>> {
    let config = run_context.run_config;
    if config.opamp.is_none() {
        debug!("No OpAMP configuration found, skipping OpAMP connectivity check");
        return Ok(());
    }

    let opamp = config.opamp;
    let base_paths = config.base_paths;
    let secret_retriever =
        OnHostSecretRetriever::new(opamp.clone(), base_paths.clone(), FileSecretProvider::new());

    let proxy = config.proxy;
    let opamp_http_builder =
        build_opamp_http_builder(opamp.clone(), proxy.clone(), secret_retriever)?
            .expect("We checked if opamp is none at the beginning");

    let file_store = Arc::new(FileStore::new_local_fs(
        base_paths.local_dir,
        base_paths.remote_dir,
    ));
    let config_repository = ConfigRepo::new(file_store.clone());
    let yaml_config_repository = Arc::new(config_repository.with_remote());

    let config_storer = Arc::new(AgentControlConfigStore::new(yaml_config_repository.clone()));
    let agent_control_config = config_storer
        .load()
        .map_err(|err| RunError(format!("failed to load Agent Control config: {err}")))?;

    let fleet_id = agent_control_config
        .fleet_control
        .as_ref()
        .map(|c| c.fleet_id.to_string())
        .unwrap_or_default();

    let http_client = HttpClient::new(HttpConfig::new(
        DEFAULT_CLIENT_TIMEOUT,
        DEFAULT_CLIENT_TIMEOUT,
        // The default value of proxy configuration is an empty proxy config without any rule
        ProxyConfig::default(),
    ))
    .map_err(|err| RunError(format!("failed to create http client: {err}")))?;
    let identifiers_provider = IdentifiersProvider::new(http_client)
        .with_host_id(agent_control_config.host_id.to_string())
        .with_fleet_id(fleet_id);
    let identifiers = identifiers_provider
        .provide()
        .map_err(|err| RunError(format!("failure obtaining identifiers: {err}")))?;
    let instance_id_storer = Storer::from(file_store);
    let instance_id_getter = InstanceIDWithIdentifiersGetter::new(instance_id_storer, identifiers);

    let client = opamp_http_builder.build().map_err(|err| {
        RunError(format!(
            "failed to build OpAMP HTTP client for connectivity check: {err}"
        ))
    })?;

    let message = AgentToServer {
        instance_uid: instance_id_getter
            .get(&AgentID::AgentControl)
            .map_err(|err| {
                RunError(format!(
                    "failed to get instance ID for OpAMP message: {err}"
                ))
            })?
            .into(),
        sequence_num: 0,
        ..Default::default()
    };

    let mut buf = Vec::new();
    message
        .encode(&mut buf)
        .map_err(|err| RunError(format!("failed to encode OpAMP message: {err}")))?;

    client
        .post(buf)
        .map_err(|err| RunError(format!("connectivity check failed: {err}")))?;

    info!("OpAMP connectivity check successful");

    Ok(())
}
