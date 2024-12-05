use crate::agent_type::variable::definition::VariableDefinition;
use crate::opamp::effective_config::loader::DefaultEffectiveConfigLoaderBuilder;
use crate::opamp::instance_id::getter::InstanceIDWithIdentifiersGetter;
use crate::opamp::instance_id::{Identifiers, Storer};
use crate::opamp::operations::build_opamp_with_channel;
use crate::sub_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
use crate::super_agent::config::AgentID;
use crate::super_agent::config_storer::loader_storer::SuperAgentConfigLoader;
use crate::super_agent::config_storer::store::SuperAgentConfigStore;
use crate::super_agent::defaults::{
    FLEET_ID_ATTRIBUTE_KEY, HOST_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY,
    OPAMP_AGENT_VERSION_ATTRIBUTE_KEY, SUB_AGENT_DIR, SUPER_AGENT_VERSION,
};
use crate::super_agent::run::SuperAgentRunner;
use crate::super_agent::{super_agent_fqn, SuperAgent};
use crate::{
    agent_type::renderer::TemplateRenderer,
    opamp::{hash_repository::on_host::HashRepositoryFile, instance_id::IdentifiersProvider},
    sub_agent::{
        on_host::builder::OnHostSubAgentBuilder,
        persister::config_persister_file::ConfigurationPersisterFile,
    },
    values::file::YAMLConfigRepositoryFile,
};
use crate::{opamp::client_builder::DefaultOpAMPClientBuilder, super_agent::error::AgentError};
use fs::directory_manager::DirectoryManagerFs;
use fs::LocalFile;
use opamp_client::operation::settings::DescriptionValueType;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

impl SuperAgentRunner {
    pub fn run(self) -> Result<(), AgentError> {
        debug!("Initialising yaml_config_repository");
        let yaml_config_repository = if self.opamp_http_builder.is_some() {
            Arc::new(
                YAMLConfigRepositoryFile::new(
                    self.base_paths.local_dir.clone(),
                    self.base_paths.remote_dir.clone(),
                )
                .with_remote(),
            )
        } else {
            Arc::new(YAMLConfigRepositoryFile::new(
                self.base_paths.local_dir.clone(),
                self.base_paths.remote_dir.clone(),
            ))
        };

        let config_storer = Arc::new(SuperAgentConfigStore::new(yaml_config_repository.clone()));
        let config = config_storer.load()?;

        let identifiers_provider = IdentifiersProvider::default()
            .with_host_id(config.host_id)
            .with_fleet_id(config.fleet_id);
        let identifiers = identifiers_provider
            .provide()
            .map_err(|e| AgentError::IdentifiersError(e.to_string()))?;
        let non_identifying_attributes = super_agent_opamp_non_identifying_attributes(&identifiers);
        info!("Instance Identifiers: {:?}", identifiers);

        let super_agent_variables = HashMap::from([(
            "host_id".to_string(),
            VariableDefinition::new_final_string_variable(identifiers.host_id.clone()),
        )]);

        let instance_id_storer = Storer::new(
            LocalFile,
            DirectoryManagerFs::default(),
            self.base_paths.remote_dir.clone(),
            self.base_paths.remote_dir.join(SUB_AGENT_DIR),
        );
        let instance_id_getter =
            InstanceIDWithIdentifiersGetter::new(instance_id_storer, identifiers);

        let super_agent_hash_repository =
            Arc::new(HashRepositoryFile::new(self.base_paths.remote_dir.clone()));
        let sub_agent_hash_repository = Arc::new(HashRepositoryFile::new(
            self.base_paths.remote_dir.join(SUB_AGENT_DIR),
        ));

        let opamp_client_builder = self.opamp_http_builder.map(|http_builder| {
            DefaultOpAMPClientBuilder::new(
                http_builder,
                DefaultEffectiveConfigLoaderBuilder::new(yaml_config_repository.clone()),
                self.opamp_poll_interval,
            )
        });

        let template_renderer = TemplateRenderer::new(self.base_paths.remote_dir.clone())
            .with_config_persister(ConfigurationPersisterFile::new(&self.base_paths.remote_dir))
            .with_super_agent_variables(super_agent_variables.into_iter());

        let agents_assembler = Arc::new(LocalEffectiveAgentsAssembler::new(
            yaml_config_repository.clone(),
            self.agent_type_registry,
            template_renderer,
        ));

        let sub_agent_builder = OnHostSubAgentBuilder::new(
            opamp_client_builder.as_ref(),
            &instance_id_getter,
            sub_agent_hash_repository,
            agents_assembler,
            self.base_paths.log_dir.join(SUB_AGENT_DIR),
            yaml_config_repository.clone(),
        );

        let (maybe_client, maybe_sa_opamp_consumer) = opamp_client_builder
            .as_ref()
            .map(|builder| {
                build_opamp_with_channel(
                    builder,
                    &instance_id_getter,
                    AgentID::new_super_agent_id(),
                    &super_agent_fqn(),
                    HashMap::from([(
                        OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
                        DescriptionValueType::String(SUPER_AGENT_VERSION.to_string()),
                    )]),
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
            self.super_agent_publisher,
            self.sub_agent_publisher,
            self.application_event_consumer,
            maybe_sa_opamp_consumer,
        )
        .run()
    }
}

pub fn super_agent_opamp_non_identifying_attributes(
    identifiers: &Identifiers,
) -> HashMap<String, DescriptionValueType> {
    HashMap::from([
        (
            HOST_NAME_ATTRIBUTE_KEY.to_string(),
            DescriptionValueType::String(identifiers.hostname.clone()),
        ),
        (
            HOST_ID_ATTRIBUTE_KEY.to_string(),
            DescriptionValueType::String(identifiers.host_id.clone()),
        ),
        (
            FLEET_ID_ATTRIBUTE_KEY.to_string(),
            DescriptionValueType::String(identifiers.fleet_id.clone()),
        ),
    ])
}
