use crate::agent_control::config_storer::loader_storer::AgentControlConfigLoader;
use crate::agent_control::config_storer::store::AgentControlConfigStore;
use crate::agent_control::config_validator::RegistryDynamicConfigValidator;
use crate::agent_control::defaults::{
    AGENT_CONTROL_VERSION, FLEET_ID_ATTRIBUTE_KEY, HOST_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY,
    OPAMP_AGENT_VERSION_ATTRIBUTE_KEY, SUB_AGENT_DIR,
};
use crate::agent_control::run::{AgentControlRunner, Environment};
use crate::agent_control::AgentControl;
use crate::agent_type::render::persister::config_persister_file::ConfigurationPersisterFile;
use crate::agent_type::render::renderer::TemplateRenderer;
use crate::agent_type::variable::definition::VariableDefinition;
use crate::http::client::HttpClient;
use crate::http::config::{HttpConfig, ProxyConfig};
use crate::opamp::effective_config::loader::DefaultEffectiveConfigLoaderBuilder;
use crate::opamp::instance_id::getter::InstanceIDWithIdentifiersGetter;
use crate::opamp::instance_id::on_host::getter::{Identifiers, IdentifiersProvider};
use crate::opamp::instance_id::on_host::storer::Storer;
use crate::opamp::operations::build_opamp_with_channel;
use crate::opamp::remote_config::validators::regexes::RegexValidator;
use crate::opamp::remote_config::validators::values::ValuesValidator;
use crate::opamp::remote_config::validators::SupportedRemoteConfigValidator;
use crate::sub_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
use crate::sub_agent::event_handler::opamp::remote_config_handler::AgentRemoteConfigHandler;
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::on_host::builder::SupervisortBuilderOnHost;
use crate::sub_agent::supervisor::assembler::AgentSupervisorAssembler;
use crate::{agent_control::error::AgentError, opamp::client_builder::DefaultOpAMPClientBuilder};
use crate::{
    opamp::hash_repository::on_host::HashRepositoryFile,
    sub_agent::on_host::builder::OnHostSubAgentBuilder, values::file::YAMLConfigRepositoryFile,
};
use fs::directory_manager::DirectoryManagerFs;
use fs::LocalFile;
use opamp_client::operation::settings::DescriptionValueType;
use resource_detection::cloud::http_client::DEFAULT_CLIENT_TIMEOUT;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

impl AgentControlRunner {
    pub(super) fn run_onhost(self) -> Result<(), AgentError> {
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

        let config_storer = Arc::new(AgentControlConfigStore::new(yaml_config_repository.clone()));
        let agent_control_config = config_storer.load()?;

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
        .map_err(|e| AgentError::Http(e.to_string()))?;

        let identifiers_provider = IdentifiersProvider::new(http_client)
            .with_host_id(agent_control_config.host_id.to_string())
            .with_fleet_id(fleet_id);

        let identifiers = identifiers_provider
            .provide()
            .map_err(|e| AgentError::Identifiers(e.to_string()))?;
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

        let agent_control_hash_repository =
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
        // Build and start AC OpAMP client
        let (maybe_client, maybe_sa_opamp_consumer) = opamp_client_builder
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
            .transpose()?
            .map(|(client, consumer)| (Some(client), Some(consumer)))
            .unwrap_or_default();

        // Disable startup check for sub-agents OpAMP client builder
        let opamp_client_builder = opamp_client_builder.map(|b| b.with_startup_check_disabled());

        let template_renderer = TemplateRenderer::default()
            .with_config_persister(
                ConfigurationPersisterFile::new(&self.base_paths.remote_dir),
                self.base_paths.remote_dir.clone(),
            )
            .with_agent_control_variables(agent_control_variables.clone().into_iter());

        let agents_assembler = Arc::new(LocalEffectiveAgentsAssembler::new(
            self.agent_type_registry.clone(),
            template_renderer,
        ));

        let supervisor_assembler = AgentSupervisorAssembler::new(
            sub_agent_hash_repository.clone(),
            SupervisortBuilderOnHost::new(self.base_paths.log_dir.join(SUB_AGENT_DIR)),
            agents_assembler,
            yaml_config_repository.clone(),
            Environment::OnHost,
        );

        // This template rendered does not include the persister to AVOID mutate any state when used to validate configs.
        let validation_renderer = TemplateRenderer::default()
            .with_agent_control_variables(agent_control_variables.into_iter());
        let validation_assembler = Arc::new(LocalEffectiveAgentsAssembler::new(
            self.agent_type_registry.clone(),
            validation_renderer,
        ));

        let remote_config_validators = vec![
            SupportedRemoteConfigValidator::Signature(self.signature_validator),
            SupportedRemoteConfigValidator::Regex(RegexValidator::default()),
            SupportedRemoteConfigValidator::Values(ValuesValidator::new(
                validation_assembler,
                Environment::OnHost,
            )),
        ];
        let remote_config_handler = AgentRemoteConfigHandler::new(
            remote_config_validators,
            sub_agent_hash_repository,
            yaml_config_repository.clone(),
        );

        let sub_agent_builder = OnHostSubAgentBuilder::new(
            opamp_client_builder.as_ref(),
            &instance_id_getter,
            Arc::new(supervisor_assembler),
            Arc::new(remote_config_handler),
        );

        let dynamic_config_validator =
            RegistryDynamicConfigValidator::new(self.agent_type_registry);

        AgentControl::new(
            maybe_client,
            agent_control_hash_repository,
            sub_agent_builder,
            config_storer,
            self.agent_control_publisher,
            self.sub_agent_publisher,
            self.application_event_consumer,
            maybe_sa_opamp_consumer,
            dynamic_config_validator,
            agent_control_config,
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
