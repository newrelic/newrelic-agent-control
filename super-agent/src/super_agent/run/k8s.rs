#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::opamp::effective_config::loader::EffectiveConfigLoaderBuilder;
use crate::opamp::instance_id::getter::InstanceIDWithIdentifiersGetter;
use crate::opamp::instance_id::Identifiers;
use crate::opamp::operations::build_opamp_with_channel;
use crate::sub_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
use crate::sub_agent::event_processor_builder::EventProcessorBuilder;
use crate::super_agent::config::AgentID;
use crate::super_agent::config_storer::loader_storer::SuperAgentConfigLoader;
use crate::super_agent::defaults::{FLEET_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY};
use crate::super_agent::{super_agent_fqn, SuperAgent};
use crate::{
    event::{
        channel::{EventConsumer, EventPublisher},
        ApplicationEvent, SuperAgentEvent,
    },
    opamp::{
        client_builder::DefaultOpAMPClientBuilder,
        hash_repository::k8s::config_map::HashRepositoryConfigMap,
        http::builder::HttpClientBuilder, instance_id,
    },
    super_agent::{config_storer::store::SuperAgentConfigStore, error::AgentError},
};
use crate::{
    k8s::{garbage_collector::NotStartedK8sGarbageCollector, store::K8sStore},
    sub_agent::{
        k8s::builder::K8sSubAgentBuilder, values::k8s::config_map::ValuesRepositoryConfigMap,
    },
    super_agent::config_storer::SubAgentsConfigStoreConfigMap,
};
use opamp_client::operation::settings::DescriptionValueType;
use resource_detection::system::hostname::HostnameGetter;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tracing::{error, info};

pub fn run_super_agent<C: HttpClientBuilder, B: EffectiveConfigLoaderBuilder>(
    runtime: Arc<Runtime>,
    sa_local_config_storer: SuperAgentConfigStore,
    application_event_consumer: EventConsumer<ApplicationEvent>,
    opamp_client_builder: Option<DefaultOpAMPClientBuilder<C, B>>,
    super_agent_publisher: EventPublisher<SuperAgentEvent>,
) -> Result<(), AgentError> {
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

pub fn super_agent_opamp_non_identifying_attributes(
    identifiers: &Identifiers,
) -> HashMap<String, DescriptionValueType> {
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
