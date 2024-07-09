use crate::opamp::effective_config::loader::EffectiveConfigLoaderBuilder;
use crate::opamp::instance_id::getter::InstanceIDWithIdentifiersGetter;
use crate::opamp::instance_id::Identifiers;
use crate::opamp::operations::build_opamp_with_channel;
use crate::sub_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
use crate::sub_agent::event_processor_builder::EventProcessorBuilder;
use crate::super_agent::config::AgentID;
use crate::super_agent::config_storer::loader_storer::SuperAgentConfigLoader;
use crate::super_agent::defaults::{
    FLEET_ID_ATTRIBUTE_KEY, HOST_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY,
};
use crate::super_agent::{super_agent_fqn, SuperAgent};
use crate::{
    agent_type::renderer::TemplateRenderer,
    opamp::{hash_repository::on_host::HashRepositoryFile, instance_id::IdentifiersProvider},
    sub_agent::{
        on_host::builder::OnHostSubAgentBuilder,
        persister::config_persister_file::ConfigurationPersisterFile,
        values::on_host::ValuesRepositoryFile,
    },
};
use crate::{
    event::{
        channel::{EventConsumer, EventPublisher},
        ApplicationEvent, SuperAgentEvent,
    },
    opamp::{client_builder::DefaultOpAMPClientBuilder, http::builder::HttpClientBuilder},
    super_agent::{config_storer::file::SuperAgentConfigStoreFile, error::AgentError},
};
use opamp_client::operation::settings::DescriptionValueType;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tracing::info;

pub fn run_super_agent<C: HttpClientBuilder, B: EffectiveConfigLoaderBuilder>(
    _runtime: Arc<Runtime>,
    sa_config_storer: SuperAgentConfigStoreFile,
    application_events_consumer: EventConsumer<ApplicationEvent>,
    opamp_client_builder: Option<DefaultOpAMPClientBuilder<C, B>>,
    super_agent_publisher: EventPublisher<SuperAgentEvent>,
) -> Result<(), AgentError> {
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
    let identifiers = identifiers_provider
        .provide()
        .map_err(|e| AgentError::IdentifiersError(e.to_string()))?;
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

pub fn super_agent_opamp_non_identifying_attributes(
    identifiers: &Identifiers,
) -> HashMap<String, DescriptionValueType> {
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
