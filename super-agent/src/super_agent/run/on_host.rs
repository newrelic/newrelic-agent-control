use crate::opamp::instance_id::getter::InstanceIDWithIdentifiersGetter;
use crate::opamp::instance_id::Identifiers;
use crate::opamp::operations::build_opamp_with_channel;
use crate::sub_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
use crate::sub_agent::event_processor_builder::EventProcessorBuilder;
use crate::super_agent::config::AgentID;
use crate::super_agent::config_storer::loader_storer::SuperAgentConfigLoader;
use crate::super_agent::defaults::{
    FLEET_ID_ATTRIBUTE_KEY, HOST_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY, LOCAL_AGENT_DATA_DIR,
    REMOTE_AGENT_DATA_DIR, SUPER_AGENT_DATA_DIR,
};
use crate::super_agent::run::SuperAgentRunner;
use crate::super_agent::{config_storer::file::SuperAgentConfigStore, error::AgentError};
use crate::super_agent::{super_agent_fqn, SuperAgent};
use crate::{
    agent_type::renderer::TemplateRenderer,
    opamp::{hash_repository::on_host::HashRepositoryFile, instance_id::IdentifiersProvider},
    sub_agent::{
        on_host::builder::OnHostSubAgentBuilder,
        persister::config_persister_file::ConfigurationPersisterFile,
    },
    values::on_host::ValuesRepositoryFile,
};
use opamp_client::operation::settings::DescriptionValueType;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

impl SuperAgentRunner {
    pub fn run_super_agent(self) -> Result<(), AgentError> {
        let mut vr_super_agent = ValuesRepositoryFile::new(
            LOCAL_AGENT_DATA_DIR().to_string(),
            REMOTE_AGENT_DATA_DIR().to_string(),
        );
        let mut vr_sub_agent = ValuesRepositoryFile::new(
            self.local_super_agent_config_path,
            SUPER_AGENT_DATA_DIR().to_string(),
        );
        if self.opamp_client_builder.is_some() {
            vr_super_agent = vr_super_agent.with_remote();
            vr_sub_agent = vr_sub_agent.with_remote();
        }
        let vr_sub_agent = Arc::new(vr_sub_agent);
        let vr_super_agent = Arc::new(vr_super_agent);

        let config_storer = Arc::new(SuperAgentConfigStore::new(vr_super_agent));

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

        let hash_repository = Arc::new(HashRepositoryFile::default());
        let agents_assembler = LocalEffectiveAgentsAssembler::new(vr_sub_agent.clone())
            .with_renderer(
                TemplateRenderer::default()
                    .with_config_persister(ConfigurationPersisterFile::default()),
            );
        let sub_agent_hash_repository = Arc::new(HashRepositoryFile::new_sub_agent_repository());
        let sub_agent_event_processor_builder =
            EventProcessorBuilder::new(sub_agent_hash_repository.clone(), vr_sub_agent.clone());

        let sub_agent_builder = OnHostSubAgentBuilder::new(
            self.opamp_client_builder.as_ref(),
            &instance_id_getter,
            sub_agent_hash_repository,
            &agents_assembler,
            &sub_agent_event_processor_builder,
            identifiers_provider,
        );

        let (maybe_client, maybe_sa_opamp_consumer) = self
            .opamp_client_builder
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
            self.super_agent_publisher,
        )
        .run(self.application_event_consumer, maybe_sa_opamp_consumer)
    }
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
