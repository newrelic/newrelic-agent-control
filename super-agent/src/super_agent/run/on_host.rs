use crate::agent_type::embedded_registry::EmbeddedRegistry;
use crate::opamp::effective_config::loader::DefaultEffectiveConfigLoaderBuilder;
use crate::opamp::instance_id::getter::InstanceIDWithIdentifiersGetter;
use crate::opamp::instance_id::{Identifiers, Storer};
use crate::opamp::operations::build_opamp_with_channel;
use crate::sub_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
use crate::sub_agent::event_processor_builder::EventProcessorBuilder;
use crate::super_agent::config::AgentID;
use crate::super_agent::config_storer::loader_storer::SuperAgentConfigLoader;
use crate::super_agent::defaults::{
    FLEET_ID_ATTRIBUTE_KEY, HOST_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY, SUB_AGENT_DIR,
};
use crate::super_agent::run::BasePaths;
use crate::super_agent::{super_agent_fqn, SuperAgent};
use crate::{
    agent_type::renderer::TemplateRenderer,
    opamp::{hash_repository::on_host::HashRepositoryFile, instance_id::IdentifiersProvider},
    sub_agent::{
        on_host::builder::OnHostSubAgentBuilder,
        persister::config_persister_file::ConfigurationPersisterFile,
    },
    values::on_host::YAMLConfigRepositoryFile,
};
use crate::{
    event::{
        channel::{EventConsumer, EventPublisher},
        ApplicationEvent, SuperAgentEvent,
    },
    opamp::{client_builder::DefaultOpAMPClientBuilder, http::builder::HttpClientBuilder},
    super_agent::{config_storer::store::SuperAgentConfigStore, error::AgentError},
};
use fs::directory_manager::DirectoryManagerFs;
use fs::LocalFile;
use opamp_client::operation::settings::DescriptionValueType;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tracing::info;

pub fn run_super_agent<C: HttpClientBuilder>(
    _runtime: Arc<Runtime>,
    sa_config_storer: SuperAgentConfigStore,
    application_events_consumer: EventConsumer<ApplicationEvent>,
    opamp_http_builder: Option<C>,
    super_agent_publisher: EventPublisher<SuperAgentEvent>,
    agent_type_registry: EmbeddedRegistry,
    base_paths: BasePaths,
) -> Result<(), AgentError> {
    // enable remote config store
    let config_storer = if opamp_http_builder.is_some() {
        Arc::new(sa_config_storer.with_remote())
    } else {
        Arc::new(sa_config_storer)
    };

    let mut vr = YAMLConfigRepositoryFile::new(
        base_paths.remote_dir.join(SUB_AGENT_DIR()),
        base_paths.local_dir.join(SUB_AGENT_DIR()),
    );
    if opamp_http_builder.is_some() {
        vr = vr.with_remote();
    }
    let yaml_config_repository = Arc::new(vr);

    let opamp_client_builder = opamp_http_builder.map(|http_builder| {
        DefaultOpAMPClientBuilder::new(
            http_builder,
            DefaultEffectiveConfigLoaderBuilder::new(yaml_config_repository.clone()),
        )
    });

    let config = config_storer.load()?;

    let identifiers_provider = IdentifiersProvider::default()
        .with_host_id(config.host_id)
        .with_fleet_id(config.fleet_id);
    let identifiers = identifiers_provider
        .provide()
        .map_err(|e| AgentError::IdentifiersError(e.to_string()))?;
    info!("Instance Identifiers: {}", identifiers);

    let non_identifying_attributes = super_agent_opamp_non_identifying_attributes(&identifiers);

    let instance_id_storer = Storer::new(
        LocalFile,
        DirectoryManagerFs::default(),
        base_paths.remote_dir.clone(),
        base_paths.remote_dir.join(SUB_AGENT_DIR()),
    );

    let instance_id_getter = InstanceIDWithIdentifiersGetter::new(instance_id_storer, identifiers);

    let template_renderer = TemplateRenderer::new(base_paths.remote_dir.clone())
        .with_config_persister(ConfigurationPersisterFile::new(&base_paths.remote_dir));

    let agents_assembler = LocalEffectiveAgentsAssembler::new(
        yaml_config_repository.clone(),
        agent_type_registry,
        template_renderer,
    );

    let super_agent_hash_repository =
        Arc::new(HashRepositoryFile::new(base_paths.remote_dir.clone()));

    let sub_agent_hash_repository = Arc::new(HashRepositoryFile::new(
        base_paths.remote_dir.join(SUB_AGENT_DIR()),
    ));

    let sub_agent_event_processor_builder = EventProcessorBuilder::new(
        sub_agent_hash_repository.clone(),
        yaml_config_repository.clone(),
    );

    let sub_agent_builder = OnHostSubAgentBuilder::new(
        opamp_client_builder.as_ref(),
        &instance_id_getter,
        sub_agent_hash_repository,
        &agents_assembler,
        &sub_agent_event_processor_builder,
        identifiers_provider,
        base_paths.log_dir.join(SUB_AGENT_DIR()),
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
        super_agent_hash_repository,
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
