use super::config_storer::store::SuperAgentConfigStore;
use super::error::AgentError;
use crate::opamp::http::builder::HttpClientBuilder;
use crate::super_agent::config_storer::loader_storer::SuperAgentConfigLoader;
use crate::{
    event::{
        channel::{EventConsumer, EventPublisher},
        ApplicationEvent, SuperAgentEvent,
    },
    opamp::client_builder::DefaultOpAMPClientBuilder,
};
use crate::{
    opamp::instance_id::Identifiers,
    super_agent::defaults::{FLEET_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY},
};
use crate::{
    opamp::{
        instance_id::getter::InstanceIDWithIdentifiersGetter, operations::build_opamp_with_channel,
    },
    sub_agent::{
        effective_agents_assembler::LocalEffectiveAgentsAssembler,
        event_processor_builder::EventProcessorBuilder,
    },
    super_agent::{config::AgentID, super_agent_fqn, SuperAgent},
};
use opamp_client::operation::settings::DescriptionValueType;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tracing::{error, info};

#[cfg(feature = "onhost")]
pub fn run_super_agent<C: HttpClientBuilder>(
    _runtime: Arc<Runtime>,
    sa_config_storer: SuperAgentConfigStore,
    application_events_consumer: EventConsumer<ApplicationEvent>,
    opamp_client_builder: Option<DefaultOpAMPClientBuilder<C>>,
    super_agent_publisher: EventPublisher<SuperAgentEvent>,
) -> Result<(), AgentError> {
    use crate::{
        agent_type::renderer::TemplateRenderer,
        opamp::{
            hash_repository::on_host::file::HashRepositoryFile, instance_id::IdentifiersProvider,
        },
        sub_agent::{
            on_host::builder::OnHostSubAgentBuilder,
            persister::config_persister_file::ConfigurationPersisterFile,
            values::on_host::file::ValuesRepositoryFile,
        },
    };

    // enable remote config store
    let config_storer = if opamp_client_builder.is_some() {
        Arc::new(sa_config_storer.with_remote())
    } else {
        Arc::new(sa_config_storer)
    };

    let config = config_storer.load()?;

    let identifiers_provider = IdentifiersProvider::default()
        .with_host_id(config.host_id)
        .with_fleet_id(config.fleet_id);
    let identifiers = identifiers_provider.provide().unwrap_or_default();
    info!("Instance Identifiers: {}", identifiers);

    let non_identifying_attributes = super_agent_opamp_non_identifying_attributes(&identifiers);

    let instance_id_getter =
        InstanceIDWithIdentifiersGetter::default().with_identifiers(identifiers);

    let mut vr = ValuesRepositoryFile::default();
    if opamp_client_builder.is_some() {
        vr = vr.with_remote();
    }
    let values_repository = Arc::new(vr);

    let hash_repository = Arc::new(HashRepositoryFile::default());
    let agents_assembler = LocalEffectiveAgentsAssembler::new(values_repository.clone())
        .with_renderer(
            TemplateRenderer::default()
                .with_config_persister(ConfigurationPersisterFile::default()),
        );
    let sub_agent_hash_repository = Arc::new(HashRepositoryFile::new_sub_agent_repository());
    let sub_agent_event_processor_builder =
        EventProcessorBuilder::new(sub_agent_hash_repository.clone(), values_repository.clone());

    let sub_agent_builder = OnHostSubAgentBuilder::new(
        opamp_client_builder.as_ref(),
        &instance_id_getter,
        sub_agent_hash_repository,
        &agents_assembler,
        &sub_agent_event_processor_builder,
        identifiers_provider,
    );

    let (maybe_client, maybe_sa_opamp_consumer) = opamp_client_builder
        .as_ref()
        .map(|builder| {
            build_opamp_with_channel(
                builder,
                &instance_id_getter,
                AgentID::new_super_agent_id(),
                &super_agent_fqn(),
                non_identifying_attributes,
            )
        })
        // Transpose changes Option<Result<T, E>> to Result<Option<T>, E>, enabling the use of `?` to handle errors in this function
        .transpose()?
        .map(|(client, consumer)| (Some(client), Some(consumer)))
        .unwrap_or_default();

    SuperAgent::new(
        maybe_client,
        hash_repository,
        sub_agent_builder,
        config_storer,
        super_agent_publisher,
    )
    .run(application_events_consumer, maybe_sa_opamp_consumer)
}

#[cfg(feature = "k8s")]
pub fn run_super_agent<C: HttpClientBuilder>(
    runtime: Arc<Runtime>,
    sa_local_config_storer: SuperAgentConfigStore,
    application_event_consumer: EventConsumer<ApplicationEvent>,
    opamp_client_builder: Option<DefaultOpAMPClientBuilder<C>>,
    super_agent_publisher: EventPublisher<SuperAgentEvent>,
) -> Result<(), AgentError> {
    #[cfg_attr(test, mockall_double::double)]
    use crate::k8s::client::SyncK8sClient;
    use crate::{
        k8s::{garbage_collector::NotStartedK8sGarbageCollector, store::K8sStore},
        opamp::{hash_repository::k8s::config_map::HashRepositoryConfigMap, instance_id},
        sub_agent::{
            k8s::builder::K8sSubAgentBuilder, values::k8s::config_map::ValuesRepositoryConfigMap,
        },
        super_agent::config_storer::SubAgentsConfigStoreConfigMap,
    };

    info!("Starting the k8s client");
    let config = sa_local_config_storer.load()?;
    let k8s_config = config.k8s.ok_or(AgentError::K8sConfig())?;
    let k8s_client = Arc::new(
        SyncK8sClient::try_new(
            runtime,
            k8s_config.namespace.clone(),
            k8s_config.cr_type_meta.clone(),
        )
        .map_err(|e| AgentError::ExternalError(e.to_string()))?,
    );

    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));

    let identifiers =
        instance_id::get_identifiers(k8s_config.cluster_name.clone(), config.fleet_id);
    info!("Instance Identifiers: {}", identifiers);

    let mut non_identifying_attributes = super_agent_opamp_non_identifying_attributes(&identifiers);
    non_identifying_attributes.insert(
        "cluster.name".to_string(),
        k8s_config.cluster_name.clone().into(),
    );

    let instance_id_getter =
        InstanceIDWithIdentifiersGetter::new_k8s_instance_id_getter(k8s_store.clone(), identifiers);

    let mut vr = ValuesRepositoryConfigMap::new(k8s_store.clone());
    if opamp_client_builder.is_some() {
        vr = vr.with_remote();
    }
    let values_repository = Arc::new(vr);

    let agents_assembler = LocalEffectiveAgentsAssembler::new(values_repository.clone());
    let hash_repository = Arc::new(HashRepositoryConfigMap::new(k8s_store.clone()));
    let sub_agent_event_processor_builder =
        EventProcessorBuilder::new(hash_repository.clone(), values_repository.clone());

    info!("Creating the k8s sub_agent builder");
    let sub_agent_builder = K8sSubAgentBuilder::new(
        opamp_client_builder.as_ref(),
        &instance_id_getter,
        k8s_client.clone(),
        hash_repository.clone(),
        &agents_assembler,
        &sub_agent_event_processor_builder,
        k8s_config.clone(),
    );

    let (maybe_client, opamp_consumer) = opamp_client_builder
        .as_ref()
        .map(|builder| {
            build_opamp_with_channel(
                builder,
                &instance_id_getter,
                AgentID::new_super_agent_id(),
                &super_agent_fqn(),
                non_identifying_attributes,
            )
        })
        // Transpose changes Option<Result<T, E>> to Result<Option<T>, E>, enabling the use of `?` to handle errors in this function
        .transpose()?
        .map(|(client, consumer)| (Some(client), Some(consumer)))
        .unwrap_or_default();

    let sub_agents_config_storer =
        SubAgentsConfigStoreConfigMap::new(k8s_store.clone(), config.dynamic);
    // enable remote config store
    let config_storer = if opamp_client_builder.is_some() {
        Arc::new(sub_agents_config_storer.with_remote())
    } else {
        Arc::new(sub_agents_config_storer)
    };

    let gcc = NotStartedK8sGarbageCollector::new(config_storer.clone(), k8s_client);
    let _started_gcc = gcc.start();

    SuperAgent::new(
        maybe_client,
        hash_repository,
        sub_agent_builder,
        config_storer,
        super_agent_publisher,
    )
    .run(application_event_consumer, opamp_consumer)
}

pub fn create_shutdown_signal_handler(
    publisher: EventPublisher<ApplicationEvent>,
) -> Result<(), ctrlc::Error> {
    ctrlc::set_handler(move || {
        info!("Received SIGINT (Ctrl-C). Stopping super agent");
        let _ = publisher
            .publish(ApplicationEvent::StopRequested)
            .map_err(|_| error!("Could not send super agent stop request"));
    })
    .map_err(|e| {
        error!("Could not set signal handler: {}", e);
        e
    })?;

    Ok(())
}

#[cfg(feature = "k8s")]
pub fn super_agent_opamp_non_identifying_attributes(
    identifiers: &Identifiers,
) -> HashMap<String, DescriptionValueType> {
    use resource_detection::system::hostname::HostnameGetter;
    let hostname = HostnameGetter::default()
        .get()
        .unwrap_or_else(|e| {
            error!("cannot retrieve hostname: {}", e.to_string());
            std::ffi::OsString::from("unknown_hostname")
        })
        .to_string_lossy()
        .to_string();

    HashMap::from([
        (
            HOST_NAME_ATTRIBUTE_KEY().to_string(),
            DescriptionValueType::String(hostname),
        ),
        (
            FLEET_ID_ATTRIBUTE_KEY().to_string(),
            DescriptionValueType::String(identifiers.fleet_id.clone()),
        ),
    ])
}

#[cfg(feature = "onhost")]
pub fn super_agent_opamp_non_identifying_attributes(
    identifiers: &Identifiers,
) -> HashMap<String, DescriptionValueType> {
    use super::defaults::HOST_ID_ATTRIBUTE_KEY;

    HashMap::from([
        (
            HOST_NAME_ATTRIBUTE_KEY().to_string(),
            DescriptionValueType::String(identifiers.hostname.clone()),
        ),
        (
            HOST_ID_ATTRIBUTE_KEY().to_string(),
            DescriptionValueType::String(identifiers.host_id.clone()),
        ),
        (
            FLEET_ID_ATTRIBUTE_KEY().to_string(),
            DescriptionValueType::String(identifiers.fleet_id.clone()),
        ),
    ])
}

#[cfg(debug_assertions)]
pub fn set_debug_dirs(cli: &crate::cli::Cli) {
    use crate::super_agent::defaults;

    if let Some(ref local_path) = cli.local_dir {
        defaults::set_local_dir(local_path);
    }
    if let Some(ref remote_path) = cli.remote_dir {
        defaults::set_remote_dir(remote_path);
    }
    if let Some(ref log_path) = cli.logs_dir {
        defaults::set_log_dir(log_path);
    }
    if let Some(ref debug_path) = cli.debug {
        defaults::set_debug_mode_dirs(debug_path);
    }
}
