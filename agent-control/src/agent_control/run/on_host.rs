//! Wiring and entry point for running Agent Control in an on-host environment.

use crate::agent_control::AgentControl;
use crate::agent_control::config::{AgentControlConfig, OpAMPClientConfig};
use crate::agent_control::config_repository::repository::AgentControlConfigLoader;
use crate::agent_control::config_validator::on_host::SharedFilesystemPathValidator;
use crate::agent_control::defaults::{
    AGENT_CONTROL_VERSION, AGENT_FILESYSTEM_FOLDER_NAME, EXECUTION_MODE_ATTRIBUTE_KEY,
    FLEET_ID_ATTRIBUTE_KEY, FOLDER_NAME_FLEET_DATA, HOST_ID_ATTRIBUTE_KEY, HOST_NAME_ATTRIBUTE_KEY,
    OPAMP_AGENT_VERSION_ATTRIBUTE_KEY, OS_ATTRIBUTE_KEY, OS_ATTRIBUTE_VALUE,
    SHARED_FILESYSTEM_FOLDER_NAME,
};
use crate::agent_control::http_server::runner::Runner;
use crate::agent_control::resource_cleaner::on_host::OnHostCleaner;
use crate::agent_control::run::{
    AgentControlRunner, GracefulShutdownReason, RunError, RunningMode,
    build_ac_opamp_start_settings, setup_config_repository_and_store,
};
use crate::agent_control::version_updater::on_host::OnHostACUpdater;
use crate::agent_control::version_updater::on_host::verify::ProcessVerifyExecutor;
use crate::agent_type::variable::Variable;
use crate::agent_type::variable::namespace::{Namespace, VariableName};
use crate::checkers::health::noop::NoOpHealthChecker;
use crate::environment::Environment;
use crate::event::channel::{EventConsumer, pub_sub};
use crate::event::{AgentControlEvent, OpAMPEvent};
use crate::http::config::ProxyConfig;
use crate::on_host::file_store::FileStore;
use crate::opamp::auth::token_retriever::TokenRetrieverImpl;
use crate::opamp::callbacks::AgentCallbacks;
use crate::opamp::client_builder::BuildOpAMPClient;
use crate::opamp::client_builder::OpAMPClientBuilder;
use crate::opamp::effective_config::loader::{EffectiveConfigLoader, EffectiveConfigLoaderBuilder};
use crate::opamp::http::builder::OpAMPHttpClientBuilder;
use crate::opamp::http::client::HttpOpAMPClient;
use crate::opamp::instance_id::getter::InstanceIDWithIdentifiersGetter;
use crate::opamp::instance_id::on_host::identifiers::{Identifiers, IdentifiersProvider};
use crate::opamp::instance_id::storer::Storer;
use crate::opamp::operations::agent_description;
use crate::opamp::remote_config::validators::SupportedRemoteConfigValidator;
use crate::opamp::remote_config::validators::regexes::RegexValidator;
use crate::opamp::secret_retriever::on_host::retrieve::OnHostSecretRetriever;
use crate::package::oci::downloader::OCIPackageArtifactDownloader;
use crate::package::oci::package_manager::OCIPackageManager;
use crate::secrets_provider::SecretsProviders;
use crate::secrets_provider::file::FileSecretProvider;
use crate::sub_agent::agent_renderer::AgentRenderer;
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::on_host::builder::OnHostSubAgentBuilder;
use crate::sub_agent::on_host::builder::SupervisorBuilderOnHost;
use crate::sub_agent::remote_config_parser::AgentRemoteConfigParser;
use crate::utils::time::SystemClock;
use crate::values::ConfigRepo;
use fs::directory_manager::DirectoryManagerFs;
use fs::file::LocalFile;
use opamp_client::http::StartedHttpClient;
use opamp_client::http::client::OpAMPHttpClient;
use opamp_client::operation::settings::{AgentDescription, DescriptionValueType, StartSettings};
use self_replacer::BinaryReplacer;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{debug, info};

/// Agent Control variable name carrying the host id.
pub const HOST_ID_VARIABLE_NAME: &str = "host_id";
/// Local OCI registry URL used by tests (debug builds only).
#[cfg(debug_assertions)]
pub const OCI_TEST_REGISTRY_URL: &str = "localhost:5001";

/// Execution environment for the on-host run mode (current target).
#[cfg(target_family = "windows")]
pub const AGENT_CONTROL_MODE_ON_HOST: Environment = Environment::Windows;
/// Execution environment for the on-host run mode (current target).
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

impl AgentControlRunner {
    /// Runs Agent Control in on-host mode until a graceful shutdown is requested.
    pub fn run_onhost(self) -> Result<GracefulShutdownReason, RunError> {
        let local_dir = self.base_paths.local_dir;
        let remote_dir = self.base_paths.remote_dir;

        // Windows-only: repair the managed data tree before anything reads/writes/deletes it (see
        // `repair_managed_permissions` and NR-601065). Not applicable on other platforms.
        #[cfg(target_family = "windows")]
        repair_managed_permissions([remote_dir.as_path(), local_dir.as_path()]);

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
            VariableName::new(Namespace::AgentControl, HOST_ID_VARIABLE_NAME),
            Variable::new_final_string_variable(identifiers.host_id.clone()),
        )]);

        let instance_id_storer = Arc::new(Storer::from(file_store));
        let instance_id_getter =
            InstanceIDWithIdentifiersGetter::new(instance_id_storer.clone(), identifiers.clone());

        let agents_package_manager = Arc::new(OCIPackageManager::new(
            OCIPackageArtifactDownloader::new(
                self.oci_client.clone(),
                self.bootstrap_config.oci.registry.clone(),
                self.bootstrap_config.oci.auth.clone(),
                agent_control_config
                    .agent_packages
                    .signature_verification_enabled
                    .into(),
            ),
            DirectoryManagerFs,
            remote_dir.clone(),
        ));

        let agent_filesystem_base = remote_dir.join(AGENT_FILESYSTEM_FOLDER_NAME);
        let fleet_data_base = remote_dir.join(FOLDER_NAME_FLEET_DATA);
        let shared_filesystem_base = remote_dir.join(SHARED_FILESYSTEM_FOLDER_NAME);
        let dir_manager = Arc::new(DirectoryManagerFs);
        let resource_cleaner = OnHostCleaner::new(
            instance_id_storer,
            yaml_config_repository.clone(),
            agent_filesystem_base,
            fleet_data_base,
            dir_manager,
            agents_package_manager.clone(),
            self.agent_type_registry.clone(),
            shared_filesystem_base,
        );
        debug!("Cleaning up resources of agents removed while Agent Control was stopped");
        resource_cleaner.cleanup_stale_agents(&agent_control_config.dynamic.agents);

        let opamp_client_builder = maybe_opamp.map(|config| {
            opamp_client_builder(
                local_dir.clone(),
                config,
                self.bootstrap_config.proxy,
                yaml_config_repository.clone(),
            )
        });

        let agent_identity = AgentIdentity::new_agent_control_identity();
        let agent_description =
            build_ac_onhost_agent_description(&agent_identity, &identifiers, RunningMode::Normal);

        self.agent_control_publisher
            .broadcast(AgentControlEvent::AgentDescriptionUpdated(
                agent_description.clone(),
            ));

        // Build and start AC OpAMP client
        let (maybe_client, maybe_sa_opamp_consumer) = opamp_client_builder
            .as_ref()
            .map(|builder| {
                let opamp_start_settings = build_ac_opamp_start_settings(
                    &instance_id_getter,
                    &agent_identity,
                    agent_description,
                    &self.dynamic_custom_capabilities,
                )?;
                start_ac_opamp_client(builder, agent_identity, opamp_start_settings)
            })
            // Transpose changes Option<Result<T, E>> to Result<Option<T>, E>, enabling the use of `?` to handle errors in this function
            .transpose()?
            .map(|(client, consumer)| (Some(client), Some(consumer)))
            .unwrap_or_default();

        let mut secrets_providers = SecretsProviders::default().with_env();
        if let Some(config) = &agent_control_config.secrets_providers {
            secrets_providers = secrets_providers
                .with_config(config.clone())
                .map_err(|e| RunError(format!("failed to load secrets providers: {e}")))?;
        }

        let agent_renderer = Arc::new(AgentRenderer::new(
            self.agent_type_registry.clone(),
            agent_control_variables,
            self.bootstrap_config.agent_type_var_constraints,
            secrets_providers,
            &remote_dir,
        ));

        let supervisor_builder = SupervisorBuilderOnHost {
            logging_base_path: self.base_paths.log_dir,
            package_manager: agents_package_manager,
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
            agent_renderer,
            sub_agent_publisher: self.sub_agent_publisher,
        };

        // Shared-filesystem conflict detection (on-host only). Resolving each agent type here also
        // surfaces unknown-type errors, so this subsumes the registry existence check on host.
        let dynamic_config_validator = SharedFilesystemPathValidator::new(self.agent_type_registry);

        // The http server stops on Drop. We need to keep it while the agent control is running.
        let _http_server = self
            .http_server_runner
            .map(Runner::start)
            .transpose()
            .map_err(|err| RunError(format!("failed to start HTTP server: {err}")))?;

        let (agent_control_internal_publisher, agent_control_internal_consumer) = pub_sub();

        let agent_control_package_manager = OCIPackageManager::new(
            OCIPackageArtifactDownloader::new(
                self.oci_client.clone(),
                self.bootstrap_config.oci.registry.clone(),
                self.bootstrap_config.oci.auth.clone(),
                agent_control_config
                    .self_update
                    .signature_verification_enabled
                    .into(),
            )
            .with_retry_policy((&agent_control_config.self_update.download_retry).into()),
            DirectoryManagerFs,
            remote_dir.clone(),
        );

        let self_replacer = match self.self_replace_target {
            Some(target) => BinaryReplacer::with_target(target),
            None => BinaryReplacer::new()
                .map_err(|e| RunError(format!("resolving self-replace target: {e}")))?,
        };

        let self_updater = OnHostACUpdater::new(
            agent_control_config.self_update.enabled.into(),
            agent_control_internal_publisher.clone(),
            agent_control_package_manager,
            ProcessVerifyExecutor::default(),
            self_replacer,
            agent_control_config.self_update.package.clone(),
            agent_control_config.self_update.upgrade_backoff.clone(),
            SystemClock,
        );

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
            resource_cleaner,
            self_updater,
            |t| Some(NoOpHealthChecker::new(t)),
            agent_control_config,
        )
        .run()
        .map_err(|err| RunError(err.to_string()))
    }
}

/// Resolves the on-host instance [`Identifiers`] (host id, hostname, fleet id) from the config.
/// Repairs Administrators-only permissions across Agent Control's managed data `roots` at startup.
///
/// An older agent-control hardened its managed directories with a NON-inheritable ACE. On upgrade
/// that wiped pre-existing runtime-created files across the data tree — sub-agent logs under
/// `filesystem/`, stored remote configs under `fleet-data/`, local data — leaving them with an
/// *empty* DACL that denies everyone, including the LocalSystem process itself. Symptoms ranged from
/// a sub-agent unable to open its log to "Access is denied" deleting a stored remote config during
/// decommission (NR-601065).
///
/// Re-stamps the Administrators-only permissions recursively over each root, repairing only entries
/// whose DACL is actually broken. Agent Control owns these files, so the rewrite succeeds even on an
/// empty DACL, and it restores read/write/delete. Best-effort: a repair failure is logged, not
/// fatal, so it never blocks startup.
///
/// Windows-only: on other platforms directory permissions are applied at creation time and there is
/// no empty-DACL failure mode, so there is nothing to repair.
#[cfg(target_family = "windows")]
fn repair_managed_permissions<'a>(roots: impl IntoIterator<Item = &'a std::path::Path>) {
    for root in roots {
        debug!("repairing managed permissions under {}", root.display());
        if let Err(err) = fs::directory_manager::ensure_permissions_recursive(root) {
            tracing::warn!(
                "repairing managed permissions under {}: {err}",
                root.display()
            );
        }
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

/// Builds the on-host OpAMP client builder from the OpAMP/proxy config and config repository.
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

/// Builds the [AgentDescription] for Agent Control on-host.
pub fn build_ac_onhost_agent_description(
    agent_identity: &AgentIdentity,
    identifiers: &Identifiers,
    running_mode: RunningMode,
) -> AgentDescription {
    agent_description(
        agent_identity,
        ac_identifying_attributes(),
        ac_non_identifying_attributes(identifiers, running_mode),
    )
}

/// Builds and starts the Agent Control on-host OpAMP client, returning it and its event consumer.
pub fn start_ac_opamp_client(
    builder: &OnHostOpAMPClientBuilder,
    agent_identity: AgentIdentity,
    settings: StartSettings,
) -> Result<(OnHostOpAMPClient, OnHostOpAMPConsumer), RunError> {
    info!("Starting Agent Control OpAMP client");
    builder
        .build_and_start(agent_identity, settings)
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
    running_mode: RunningMode,
) -> HashMap<String, DescriptionValueType> {
    let mut attributes = HashMap::from([
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
    ]);

    // Only add execution mode attribute in verify mode
    if running_mode == RunningMode::Verify {
        attributes.insert(
            EXECUTION_MODE_ATTRIBUTE_KEY.to_string(),
            "dry-run".to_string().into(),
        );
    }

    attributes
}
