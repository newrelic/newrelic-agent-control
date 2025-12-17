use crate::agent_control::AgentControl;
use crate::agent_control::config_repository::repository::AgentControlConfigLoader;
use crate::agent_control::config_repository::store::AgentControlConfigStore;
use crate::agent_control::config_validator::RegistryDynamicConfigValidator;
use crate::agent_control::defaults::{
    AGENT_CONTROL_VERSION, FLEET_ID_ATTRIBUTE_KEY, HOST_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY,
    OPAMP_AGENT_VERSION_ATTRIBUTE_KEY, OS_ATTRIBUTE_KEY, OS_ATTRIBUTE_VALUE,
};
use crate::agent_control::http_server::runner::Runner;
use crate::agent_control::resource_cleaner::no_op::NoOpResourceCleaner;
use crate::agent_control::run::{AgentControlRunner, Environment, RunError};
use crate::agent_control::version_updater::updater::NoOpUpdater;
use crate::agent_type::render::TemplateRenderer;
use crate::agent_type::variable::Variable;
use crate::checkers::health::noop::NoOpHealthChecker;
use crate::event::channel::pub_sub;
use crate::http::client::HttpClient;
use crate::http::config::{HttpConfig, ProxyConfig};
use crate::on_host::file_store::FileStore;
use crate::opamp::client_builder::DefaultOpAMPClientBuilder;
use crate::opamp::effective_config::loader::DefaultEffectiveConfigLoaderBuilder;
use crate::opamp::instance_id::getter::InstanceIDWithIdentifiersGetter;
use crate::opamp::instance_id::on_host::identifiers::{Identifiers, IdentifiersProvider};
use crate::opamp::instance_id::storer::Storer;
use crate::opamp::operations::build_opamp_with_channel;
use crate::opamp::remote_config::validators::SupportedRemoteConfigValidator;
use crate::opamp::remote_config::validators::regexes::RegexValidator;
use crate::secret_retriever::on_host::retrieve::OnHostSecretRetriever;
use crate::secrets_provider::SecretsProviders;
use crate::secrets_provider::file::FileSecretProvider;
use crate::sub_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::on_host::builder::OnHostSubAgentBuilder;
use crate::sub_agent::on_host::builder::SupervisortBuilderOnHost;
use crate::sub_agent::remote_config_parser::AgentRemoteConfigParser;
use crate::values::ConfigRepo;
use opamp_client::operation::settings::DescriptionValueType;
use resource_detection::cloud::http_client::DEFAULT_CLIENT_TIMEOUT;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{debug, info};

pub const HOST_ID_VARIABLE_NAME: &str = "host_id";

#[cfg(target_family = "windows")]
pub const AGENT_CONTROL_MODE_ON_HOST: Environment = Environment::Windows;
#[cfg(target_family = "unix")]
pub const AGENT_CONTROL_MODE_ON_HOST: Environment = Environment::Linux;

impl AgentControlRunner {
    pub(super) fn run_onhost(self) -> Result<(), RunError> {
        let file_store = Arc::new(FileStore::new_local_fs(
            self.base_paths.local_dir.clone(),
            self.base_paths.remote_dir.clone(),
        ));

        let secret_retriever = OnHostSecretRetriever::new(
            self.opamp.clone(),
            self.base_paths.clone(),
            FileSecretProvider::new(),
        );

        let opamp_http_builder =
            Self::build_opamp_http_builder(self.opamp, self.proxy.clone(), secret_retriever)?;

        debug!("Initializing yaml_config_repository");
        let config_repository = ConfigRepo::new(file_store.clone());
        let yaml_config_repository = Arc::new(if opamp_http_builder.is_some() {
            config_repository.with_remote()
        } else {
            config_repository
        });

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
        let non_identifying_attributes =
            agent_control_opamp_non_identifying_attributes(&identifiers);
        info!("Instance Identifiers: {:?}", identifiers);

        let agent_control_variables = HashMap::from([(
            HOST_ID_VARIABLE_NAME.to_string(),
            Variable::new_final_string_variable(identifiers.host_id.clone()),
        )]);

        let instance_id_storer = Storer::from(file_store);
        let instance_id_getter =
            InstanceIDWithIdentifiersGetter::new(instance_id_storer, identifiers);

        let opamp_client_builder = opamp_http_builder.map(|http_builder| {
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
            .transpose()
            .map_err(|err| RunError(format!("error initializing OpAMP client: {err}")))?
            .map(|(client, consumer)| (Some(client), Some(consumer)))
            .unwrap_or_default();

        // Disable startup check for sub-agents OpAMP client builder
        let opamp_client_builder = opamp_client_builder.map(|b| b.with_startup_check_disabled());

        let template_renderer = TemplateRenderer::default()
            .with_agent_control_variables(agent_control_variables.clone().into_iter());

        let mut secrets_providers = SecretsProviders::default().with_env();
        if let Some(config) = &agent_control_config.secrets_providers {
            secrets_providers = secrets_providers
                .with_config(config.clone())
                .map_err(|e| RunError(format!("failed to load secrets providers: {e}")))?;
        }

        let agents_assembler = Arc::new(LocalEffectiveAgentsAssembler::new(
            self.agent_type_registry.clone(),
            template_renderer,
            self.agent_type_var_constraints,
            secrets_providers,
            &self.base_paths.remote_dir,
        ));

        let supervisor_builder = SupervisortBuilderOnHost::new(self.base_paths.log_dir);

        let signature_validator = Arc::new(self.signature_validator);
        let remote_config_validators = vec![
            SupportedRemoteConfigValidator::Signature(signature_validator.clone()),
            SupportedRemoteConfigValidator::Regex(RegexValidator::default()),
        ];
        let remote_config_parser = AgentRemoteConfigParser::new(remote_config_validators);

        let sub_agent_builder = OnHostSubAgentBuilder {
            opamp_builder: opamp_client_builder.as_ref(),
            instance_id_getter: &instance_id_getter,
            supervisor_builder: Arc::new(supervisor_builder),
            remote_config_parser: Arc::new(remote_config_parser),
            yaml_config_repository,
            effective_agents_assembler: agents_assembler,
            sub_agent_publisher: self.sub_agent_publisher,
            ac_running_mode: self.ac_running_mode,
        };

        let dynamic_config_validator =
            RegistryDynamicConfigValidator::new(self.agent_type_registry);

        // The http server stops on Drop. We need to keep it while the agent control is running.
        let _http_server = self.http_server_runner.map(Runner::start);

        let (agent_control_internal_publisher, agent_control_internal_consumer) = pub_sub();
        AgentControl::new(
            maybe_client,
            sub_agent_builder,
            SystemTime::now(),
            config_storer,
            self.agent_control_publisher,
            self.application_event_consumer,
            maybe_sa_opamp_consumer,
            agent_control_internal_publisher,
            agent_control_internal_consumer,
            SupportedRemoteConfigValidator::Signature(signature_validator),
            dynamic_config_validator,
            NoOpResourceCleaner,
            NoOpUpdater,
            |t| Some(NoOpHealthChecker::new(t)),
            agent_control_config,
        )
        .run()
        .map_err(|err| RunError(err.to_string()))
    }
}
pub fn agent_control_opamp_non_identifying_attributes(
    identifiers: &Identifiers,
) -> HashMap<String, DescriptionValueType> {
    HashMap::from([
        (
            HOST_NAME_ATTRIBUTE_KEY.to_string(),
            identifiers.hostname.clone().into(),
        ),
        (
            HOST_ID_ATTRIBUTE_KEY.to_string(),
            identifiers.host_id.clone().into(),
        ),
        (
            FLEET_ID_ATTRIBUTE_KEY.to_string(),
            identifiers.fleet_id.clone().into(),
        ),
        (
            OS_ATTRIBUTE_KEY.to_string(),
            OS_ATTRIBUTE_VALUE.to_string().into(),
        ),
    ])
}
