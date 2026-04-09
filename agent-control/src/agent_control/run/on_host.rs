use crate::agent_control::AgentControl;
use crate::agent_control::config::{AgentControlConfig, OpAMPClientConfig};
use crate::agent_control::config_repository::repository::AgentControlConfigLoader;
use crate::agent_control::config_validator::RegistryDynamicConfigValidator;
use crate::agent_control::defaults::{
    AGENT_CONTROL_VERSION, FLEET_ID_ATTRIBUTE_KEY, HOST_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY,
    OPAMP_AGENT_VERSION_ATTRIBUTE_KEY, OS_ATTRIBUTE_KEY, OS_ATTRIBUTE_VALUE,
};
use crate::agent_control::http_server::runner::Runner;
use crate::agent_control::resource_cleaner::no_op::NoOpResourceCleaner;
use crate::agent_control::run::{
    AgentControlRunner, Environment, RunError, setup_config_repository_and_store,
};
use crate::agent_control::version_updater::on_host::ProcessVerifyExecutor;
use crate::agent_control::version_updater::updater::OnHostACUpdater;
use crate::agent_type::render::TemplateRenderer;
use crate::agent_type::variable::Variable;
use crate::checkers::health::noop::NoOpHealthChecker;
use crate::event::OpAMPEvent;
use crate::event::channel::{EventConsumer, pub_sub};
use crate::http::config::ProxyConfig;
use crate::oci;
use crate::on_host::file_store::FileStore;
use crate::opamp::auth::token_retriever::TokenRetrieverImpl;
use crate::opamp::callbacks::AgentCallbacks;
use crate::opamp::client_builder::BuildOpAMPClient;
use crate::opamp::client_builder::OpAMPClientBuilder;
use crate::opamp::effective_config::loader::{EffectiveConfigLoader, EffectiveConfigLoaderBuilder};
use crate::opamp::http::builder::OpAMPHttpClientBuilder;
use crate::opamp::http::client::HttpOpAMPClient;
use crate::opamp::instance_id::getter::{InstanceIDGetter, InstanceIDWithIdentifiersGetter};
use crate::opamp::instance_id::on_host::identifiers::{Identifiers, IdentifiersProvider};
use crate::opamp::instance_id::storer::Storer;
use crate::opamp::operations::start_settings;
use crate::opamp::remote_config::validators::SupportedRemoteConfigValidator;
use crate::opamp::remote_config::validators::regexes::RegexValidator;
use crate::package::oci::downloader::OCIArtifactDownloader;
use crate::package::oci::package_manager::OCIPackageManager;
use crate::secret_retriever::on_host::retrieve::OnHostSecretRetriever;
use crate::secrets_provider::SecretsProviders;
use crate::secrets_provider::file::FileSecretProvider;
use crate::sub_agent::effective_agents_assembler::LocalEffectiveAgentsAssembler;
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::on_host::builder::OnHostSubAgentBuilder;
use crate::sub_agent::on_host::builder::SupervisorBuilderOnHost;
use crate::sub_agent::remote_config_parser::AgentRemoteConfigParser;
use crate::values::ConfigRepo;
use fs::directory_manager::DirectoryManagerFs;
use fs::file::LocalFile;
use oci_client::client::ClientConfig;
#[cfg(debug_assertions)]
use oci_client::client::ClientProtocol;
use opamp_client::http::StartedHttpClient;
use opamp_client::http::client::OpAMPHttpClient;
use opamp_client::operation::settings::DescriptionValueType;
use self_replacer::unix::MockUNIXSelfReplacer;
#[cfg(target_family = "windows")]
use self_replacer::windows::WindowsSelfReplacer;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::info;

pub const HOST_ID_VARIABLE_NAME: &str = "host_id";
#[cfg(debug_assertions)]
pub const OCI_TEST_REGISTRY_URL: &str = "localhost:5001";

#[cfg(target_family = "windows")]
pub const AGENT_CONTROL_MODE_ON_HOST: Environment = Environment::Windows;
#[cfg(target_family = "unix")]
pub const AGENT_CONTROL_MODE_ON_HOST: Environment = Environment::Linux;

type OnHostOpAMPClientBuilder = OpAMPClientBuilder<
    OpAMPHttpClientBuilder<OnHostSecretRetriever<FileSecretProvider>>,
    EffectiveConfigLoaderBuilder<ConfigRepo<FileStore<LocalFile, DirectoryManagerFs>>>,
>;
type OnHostOpAMPClient = StartedHttpClient<
    OpAMPHttpClient<
        AgentCallbacks<EffectiveConfigLoader<ConfigRepo<FileStore<LocalFile, DirectoryManagerFs>>>>,
        HttpOpAMPClient<TokenRetrieverImpl>,
    >,
>;
type OnHostOpAMPConsumer = EventConsumer<OpAMPEvent>;
type OnHostInstanceIdGetter =
    InstanceIDWithIdentifiersGetter<Storer<FileStore<LocalFile, DirectoryManagerFs>, Identifiers>>;

impl AgentControlRunner {
    pub fn run_onhost(self) -> Result<(), RunError> {
        let on_host_config = self.bootstrap_config.onhost.clone().unwrap_or_default();

        let local_dir = self.base_paths.local_dir;
        let remote_dir = self.base_paths.remote_dir;
        let file_store = Arc::new(FileStore::new_local_fs(
            local_dir.clone(),
            remote_dir.clone(),
        ));

        let maybe_opamp = self.bootstrap_config.fleet_control;

        let (yaml_config_repository, config_storer) =
            setup_config_repository_and_store(file_store.clone(), maybe_opamp.is_some());
        let agent_control_config = config_storer
            .load()
            .map_err(|err| RunError(format!("failed to load Agent Control config: {err}")))?;

        let identifiers = ac_identifiers(&agent_control_config)?;

        let agent_control_variables = HashMap::from([(
            HOST_ID_VARIABLE_NAME.to_string(),
            Variable::new_final_string_variable(identifiers.host_id.clone()),
        )]);

        let instance_id_storer = Storer::from(file_store);
        let instance_id_getter =
            InstanceIDWithIdentifiersGetter::new(instance_id_storer, identifiers.clone());

        let proxy = self.bootstrap_config.proxy;
        let opamp_client_builder = maybe_opamp.map(|config| {
            opamp_client_builder(
                local_dir.clone(),
                config,
                proxy.clone(),
                yaml_config_repository.clone(),
            )
        });

        // Build and start AC OpAMP client
        let (maybe_client, maybe_sa_opamp_consumer) = opamp_client_builder
            .as_ref()
            .map(|builder| start_ac_opamp_client(builder, &instance_id_getter, &identifiers))
            // Transpose changes Option<Result<T, E>> to Result<Option<T>, E>, enabling the use of `?` to handle errors in this function
            .transpose()?
            .map(|(client, consumer)| (Some(client), Some(consumer)))
            .unwrap_or_default();

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
            self.bootstrap_config.agent_type_var_constraints,
            secrets_providers,
            &remote_dir,
        ));

        // We are setting client http in debug_assertions mode for tests
        let oci_client_config = ClientConfig {
            #[cfg(debug_assertions)]
            protocol: ClientProtocol::HttpsExcept(vec![OCI_TEST_REGISTRY_URL.to_string()]),
            ..Default::default()
        };

        let oci_client = oci::Client::try_new(oci_client_config, proxy, self.runtime.clone())
            .map_err(|err| RunError(format!("failed to create the OciClient: {err}")))?;

        let packages_downloader = OCIArtifactDownloader::new(
            oci_client,
            agent_control_config
                .agent_packages
                .signature_verification_enabled
                .into(),
        );

        let package_manager = Arc::new(OCIPackageManager::new(
            packages_downloader,
            DirectoryManagerFs,
            remote_dir.clone(),
        ));

        let supervisor_builder = SupervisorBuilderOnHost {
            logging_path: self.base_paths.log_dir,
            package_manager: package_manager.clone(),
        };

        let signature_validator = Arc::new(self.signature_validator);
        let remote_config_validators = vec![
            SupportedRemoteConfigValidator::Signature(signature_validator.clone()),
            SupportedRemoteConfigValidator::Regex(RegexValidator::default()),
        ];
        let remote_config_parser = AgentRemoteConfigParser::new(remote_config_validators);

        let opamp_builder =
            opamp_client_builder.map(|builder| builder.with_startup_check_disabled());

        let sub_agent_builder = OnHostSubAgentBuilder {
            opamp_builder,
            instance_id_getter,
            supervisor_builder: Arc::new(supervisor_builder),
            remote_config_parser: Arc::new(remote_config_parser),
            yaml_config_repository,
            effective_agents_assembler: agents_assembler,
            sub_agent_publisher: self.sub_agent_publisher,
            ac_running_mode: self.running_mode,
        };

        let dynamic_config_validator =
            RegistryDynamicConfigValidator::new(self.agent_type_registry);

        // The http server stops on Drop. We need to keep it while the agent control is running.
        let _http_server = self
            .http_server_runner
            .map(Runner::start)
            .transpose()
            .map_err(|err| RunError(format!("failed to start HTTP server: {err}")))?;

        let (agent_control_internal_publisher, agent_control_internal_consumer) = pub_sub();

        #[cfg(target_family = "windows")]
        let updater = OnHostACUpdater {
            ac_remote_update_enabled: on_host_config.ac_remote_update,
            agent_control_internal_publisher: agent_control_internal_publisher.clone(),
            self_replacer: WindowsSelfReplacer,
            verify_executor: ProcessVerifyExecutor::default(),
            package_manager,
        };

        #[cfg(target_family = "unix")]
        let updater = OnHostACUpdater {
            ac_remote_update_enabled: on_host_config.ac_remote_update,
            agent_control_internal_publisher: agent_control_internal_publisher.clone(),
            self_replacer: MockUNIXSelfReplacer,
            verify_executor: ProcessVerifyExecutor::default(),
            package_manager,
        };

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
            updater,
            |t| Some(NoOpHealthChecker::new(t)),
            agent_control_config,
        )
        .run()
        .map_err(|err| RunError(err.to_string()))
    }
}

pub fn ac_identifiers(config: &AgentControlConfig) -> Result<Identifiers, RunError> {
    let fleet_id = config
        .fleet_control
        .as_ref()
        .map(|c| c.fleet_id.to_string())
        .unwrap_or_default();

    let identifiers_provider = IdentifiersProvider::try_default()
        .map_err(|err| RunError(format!("failed to build the identifiers provider: {err}")))?
        .with_host_id(config.host_id.to_string())
        .with_fleet_id(fleet_id);

    let identifiers = identifiers_provider
        .provide()
        .map_err(|err| RunError(format!("failure obtaining identifiers: {err}")))?;
    info!("Instance Identifiers: {:?}", identifiers);

    Ok(identifiers)
}

pub fn opamp_client_builder(
    local_dir: PathBuf,
    opamp_config: OpAMPClientConfig,
    proxy_config: ProxyConfig,
    yaml_config_repository: Arc<ConfigRepo<FileStore<LocalFile, DirectoryManagerFs>>>,
) -> OnHostOpAMPClientBuilder {
    let secret_retriever = OnHostSecretRetriever::new(
        Some(opamp_config.clone()),
        local_dir.clone(),
        FileSecretProvider::new(),
    );

    let poll_interval = opamp_config.poll_interval;
    let http_builder = OpAMPHttpClientBuilder::new(opamp_config, proxy_config, secret_retriever);
    let loader = EffectiveConfigLoaderBuilder::new(yaml_config_repository.clone());

    OpAMPClientBuilder::new(poll_interval, http_builder, loader)
}

pub fn start_ac_opamp_client(
    builder: &OnHostOpAMPClientBuilder,
    instance_id_getter: &OnHostInstanceIdGetter,
    identifiers: &Identifiers,
) -> Result<(OnHostOpAMPClient, OnHostOpAMPConsumer), RunError> {
    info!("Starting Agent Control OpAMP client");

    let agent_identity = AgentIdentity::new_agent_control_identity();
    let instance_id = instance_id_getter
        .get(&agent_identity.id)
        .map_err(|err| RunError(format!("error getting instance id: {err}")))?;

    let agent_identity = AgentIdentity::new_agent_control_identity();
    let start_settings = start_settings(
        instance_id,
        &agent_identity,
        ac_identifying_attributes(),
        ac_non_identifying_attributes(identifiers),
    );

    builder
        .build_and_start(agent_identity, start_settings)
        .map_err(|err| RunError(format!("error initializing OpAMP client: {err}")))
}

fn ac_identifying_attributes() -> HashMap<String, DescriptionValueType> {
    HashMap::from([(
        OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
        DescriptionValueType::String(AGENT_CONTROL_VERSION.to_string()),
    )])
}

fn ac_non_identifying_attributes(
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
