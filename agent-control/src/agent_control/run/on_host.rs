use crate::agent_control::config::{AgentID, AgentTypeFQN};
use crate::agent_control::config_storer::loader_storer::AgentControlConfigLoader;
use crate::agent_control::config_storer::store::AgentControlConfigStore;
use crate::agent_control::defaults::{
    AGENT_CONTROL_VERSION, FLEET_ID_ATTRIBUTE_KEY, HOST_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY,
    OPAMP_AGENT_VERSION_ATTRIBUTE_KEY, SUB_AGENT_DIR,
};
use crate::agent_control::run::AgentControlRunner;
use crate::agent_control::AgentControl;
use crate::agent_type::variable::definition::VariableDefinition;
use crate::opamp::effective_config::loader::DefaultEffectiveConfigLoaderBuilder;
use crate::opamp::instance_id::getter::InstanceIDWithIdentifiersGetter;
use crate::opamp::instance_id::{Identifiers, Storer};
use crate::opamp::operations::build_opamp_with_channel;
use crate::opamp::remote_config::status_manager::local_filesystem::FileSystemConfigStatusManager;
use crate::opamp::remote_config::validators::signature::validator::{
    build_signature_validator, SignatureValidator,
};
use crate::sub_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
use crate::{agent_control::error::AgentError, opamp::client_builder::DefaultOpAMPClientBuilder};
use crate::{
    agent_type::renderer::TemplateRenderer,
    opamp::instance_id::IdentifiersProvider,
    sub_agent::{
        on_host::builder::OnHostSubAgentBuilder,
        persister::config_persister_file::ConfigurationPersisterFile,
    },
};
use fs::directory_manager::DirectoryManagerFs;
use fs::LocalFile;
use opamp_client::operation::settings::DescriptionValueType;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

impl AgentControlRunner {
    pub fn run(self) -> Result<(), AgentError> {
        debug!("Initialising AC config manager");

        let config_manager = FileSystemConfigStatusManager::new(self.base_paths.local_dir.clone());
        let config_manager = if self.opamp_http_builder.is_some() {
            Arc::new(config_manager.with_remote(self.base_paths.remote_dir.clone()))
        } else {
            Arc::new(config_manager)
        };

        let config_storer = Arc::new(AgentControlConfigStore::new(config_manager.clone()));
        let config = config_storer.load()?;

        let fleet_id = config
            .fleet_control
            .as_ref()
            .map(|c| c.fleet_id.clone())
            .unwrap_or_default();

        let identifiers_provider = IdentifiersProvider::default()
            .with_host_id(config.host_id.clone())
            .with_fleet_id(fleet_id);

        let identifiers = identifiers_provider
            .provide()
            .map_err(|e| AgentError::IdentifiersError(e.to_string()))?;
        let non_identifying_attributes =
            agent_control_opamp_non_identifying_attributes(&identifiers);
        info!("Instance Identifiers: {:?}", identifiers);

        let agent_control_variables = HashMap::from([(
            "host_id".to_string(),
            VariableDefinition::new_final_string_variable(identifiers.host_id.clone()),
        )]);

        let instance_id_storer = Storer::new(
            LocalFile,
            DirectoryManagerFs,
            self.base_paths.remote_dir.clone(),
            self.base_paths.remote_dir.join(SUB_AGENT_DIR),
        );
        let instance_id_getter =
            InstanceIDWithIdentifiersGetter::new(instance_id_storer, identifiers);

        let opamp_client_builder = self.opamp_http_builder.map(|http_builder| {
            DefaultOpAMPClientBuilder::new(
                http_builder,
                DefaultEffectiveConfigLoaderBuilder::new(config_manager.clone()),
                self.opamp_poll_interval,
            )
        });

        let template_renderer = TemplateRenderer::new(self.base_paths.remote_dir.clone())
            .with_config_persister(ConfigurationPersisterFile::new(&self.base_paths.remote_dir))
            .with_agent_control_variables(agent_control_variables.into_iter());

        let agents_assembler = Arc::new(LocalEffectiveAgentsAssembler::new(
            config_manager.clone(),
            self.agent_type_registry,
            template_renderer,
        ));

        let signature_validator = config
            .fleet_control
            .map(|fleet_config| {
                build_signature_validator(fleet_config.signature_validation).map_err(|e| {
                    AgentError::ExternalError(format!("initializing signature validator: {}", e))
                })
            })
            .transpose()?
            .unwrap_or(SignatureValidator::Noop);

        let sub_agent_builder = OnHostSubAgentBuilder::new(
            opamp_client_builder.as_ref(),
            &instance_id_getter,
            agents_assembler,
            self.base_paths.log_dir.join(SUB_AGENT_DIR),
            Arc::new(signature_validator),
            config_manager,
        );

        let (maybe_client, maybe_sa_opamp_consumer) = opamp_client_builder
            .as_ref()
            .map(|builder| {
                build_opamp_with_channel(
                    builder,
                    &instance_id_getter,
                    AgentID::new_agent_control_id(),
                    &AgentTypeFQN::new_agent_control_fqn(),
                    HashMap::from([(
                        OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
                        DescriptionValueType::String(AGENT_CONTROL_VERSION.to_string()),
                    )]),
                    non_identifying_attributes,
                )
            })
            // Transpose changes Option<Result<T, E>> to Result<Option<T>, E>, enabling the use of `?` to handle errors in this function
            .transpose()?
            .map(|(client, consumer)| (Some(client), Some(consumer)))
            .unwrap_or_default();
        AgentControl::new(
            maybe_client,
            sub_agent_builder,
            config_storer,
            self.agent_control_publisher,
            self.sub_agent_publisher,
            self.application_event_consumer,
            maybe_sa_opamp_consumer,
        )
        .run()
    }
}

pub fn agent_control_opamp_non_identifying_attributes(
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
