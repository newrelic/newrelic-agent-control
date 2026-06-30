//! Entry points and shared wiring for running Agent Control on-host and in Kubernetes.

use super::defaults::{
    AGENT_CONTROL_DATA_DIR, AGENT_CONTROL_LOCAL_DATA_DIR, AGENT_CONTROL_LOG_DIR,
    DYNAMIC_AGENT_TYPES_DIR,
};
use crate::agent_control::config::{AgentControlConfig, AgentControlConfigError};
use crate::agent_control::config_repository::store::AgentControlConfigStore;
use crate::agent_control::http_server::runner::Runner;
use crate::agent_type::oci::downloader::OCIAgentTypeArtifactDownloader;
use crate::agent_type::registry::{Registry, RegistryConfig};
use crate::command::RunnerContext;
use crate::data_store::DataStore;
use crate::event::broadcaster::unbounded::UnboundedBroadcast;
use crate::event::{AgentControlEvent, ApplicationEvent, SubAgentEvent, channel::EventConsumer};
use crate::oci;
use crate::opamp::remote_config::validators::signature::validator::SignatureValidator;
use crate::values::ConfigRepo;
use oci_client::client::ClientConfig;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tracing::debug;

/// Error returned when running Agent Control fails.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct RunError(String);

/// The reason Agent Control stopped gracefully.
#[derive(Debug, PartialEq)]
pub enum GracefulShutdownReason {
    /// A stop was requested externally (SIGTERM, SCM stop).
    ExternalRequested,
    /// A successful self-update occurred, and the process needs to be restarted to apply the update.
    SelfUpdate,
}

// k8s and on_host need to be public to allow integration tests to access the fn run_agent_control.

pub mod k8s;
pub mod on_host;

/// Defines the execution mode of Agent Control
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum RunningMode {
    /// Normal long-running process mode
    Normal,
    /// Verification mode (dry-run check)
    Verify,
}

/// Structure with all base paths required to run Agent Control
#[derive(Debug, Clone)]
pub struct BasePaths {
    /// Directory holding local (non-remote) configuration and data.
    pub local_dir: PathBuf,
    /// Directory holding remote (fleet) data.
    pub remote_dir: PathBuf,
    /// Directory holding log files.
    pub log_dir: PathBuf,
}

impl Default for BasePaths {
    fn default() -> Self {
        Self {
            local_dir: PathBuf::from(AGENT_CONTROL_LOCAL_DATA_DIR),
            remote_dir: PathBuf::from(AGENT_CONTROL_DATA_DIR),
            log_dir: PathBuf::from(AGENT_CONTROL_LOG_DIR),
        }
    }
}

/// Structure with all the data required to run the agent control.
pub struct AgentControlRunner {
    /// Config loaded at startup from local files. Used to bootstrap
    /// the runner before the platform-specific (on-host/k8s) store is available.
    /// Environment-specific `run()` methods re-load config from their
    /// respective stores (file for on-host / ConfigMap for k8s) to get the corresponding config
    /// including remote configuration when needed.
    bootstrap_config: AgentControlConfig,

    agent_type_registry: Arc<Registry>,
    oci_client: oci::Client,
    application_event_consumer: EventConsumer<ApplicationEvent>,
    agent_control_publisher: UnboundedBroadcast<AgentControlEvent>,
    sub_agent_publisher: UnboundedBroadcast<SubAgentEvent>,
    signature_validator: SignatureValidator,
    base_paths: BasePaths,
    runtime: Arc<Runtime>,
    http_server_runner: Option<Runner>,
    self_replace_target: Option<PathBuf>,
}

impl AgentControlRunner {
    /// Builds the runner from a [`RunnerContext`], setting up the async runtime, OCI client,
    /// agent type registry, signature validator and (optionally) the status HTTP server.
    pub fn try_new(context: RunnerContext) -> Result<Self, Box<dyn Error>> {
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?,
        );

        let mut agent_control_publisher = UnboundedBroadcast::default();
        let mut sub_agent_publisher = UnboundedBroadcast::default();
        let http_server_runner = context.bootstrap_config.server.enabled.then(|| {
            let agent_control_consumer = EventConsumer::from(agent_control_publisher.subscribe());
            let sub_agent_consumer = EventConsumer::from(sub_agent_publisher.subscribe());
            Runner::new(
                context.bootstrap_config.server.clone(),
                runtime.clone(),
                agent_control_consumer,
                sub_agent_consumer,
                context.bootstrap_config.fleet_control.clone(),
            )
        });

        // We are setting client http in debug_assertions mode for tests
        let oci_client_config = ClientConfig {
            #[cfg(debug_assertions)]
            protocol: oci_client::client::ClientProtocol::HttpsExcept(vec![
                crate::agent_control::run::on_host::OCI_TEST_REGISTRY_URL.to_string(),
            ]),
            ..Default::default()
        };
        let oci_client = oci::Client::try_new(
            oci_client_config,
            context.bootstrap_config.proxy.clone(),
            runtime.clone(),
        )?;

        let agent_type_registry =
            Arc::new(build_agent_type_registry(&context, oci_client.clone())?);

        let signature_validator = context
            .bootstrap_config
            .fleet_control
            .clone()
            .map(|fleet_config| {
                SignatureValidator::new(
                    fleet_config.signature_validation,
                    context.bootstrap_config.proxy.clone(),
                )
            })
            .transpose()?
            .unwrap_or(SignatureValidator::new_noop());

        Ok(AgentControlRunner {
            bootstrap_config: context.bootstrap_config,
            http_server_runner,
            runtime,
            agent_type_registry,
            oci_client,
            application_event_consumer: context.application_event_consumer,
            agent_control_publisher,
            sub_agent_publisher,
            base_paths: context.base_paths,
            signature_validator,
            self_replace_target: context.self_replace_target,
        })
    }
}

fn build_agent_type_registry(
    context: &RunnerContext,
    oci_client: oci::Client,
) -> Result<Registry, AgentControlConfigError> {
    let default_remote = &context.bootstrap_config.agent_types.default_remote;
    let signature_verification_enabled = default_remote.signature_verification_enabled.into();
    let default_public_key_url = &default_remote.public_key_url;

    if signature_verification_enabled && default_public_key_url.as_str().is_empty() {
        return Err(AgentControlConfigError(
            "Signature verification is enabled, but public_key_url is empty".to_string(),
        ));
    }

    let public_key_url = signature_verification_enabled.then(|| default_public_key_url.clone());

    let downloader = OCIAgentTypeArtifactDownloader::new(
        oci_client,
        context.bootstrap_config.oci.registry.clone(),
        default_remote.repository.clone(),
        context.bootstrap_config.oci.auth.clone(),
        public_key_url,
    );

    Ok(Registry::build(
        context.running_mode,
        RegistryConfig {
            dynamic_agent_types_path: context.base_paths.local_dir.join(DYNAMIC_AGENT_TYPES_DIR),
        },
        downloader,
    ))
}

type RepositoryAndStore<D> = (
    Arc<ConfigRepo<D>>,
    Arc<AgentControlConfigStore<ConfigRepo<D>>>,
);

/// Helper to handle configuration repository and store for all running modes.
pub fn setup_config_repository_and_store<D: DataStore + Send + Sync + 'static>(
    data_store: Arc<D>,
    with_remote: bool,
) -> RepositoryAndStore<D> {
    debug!("Initializing yaml_config_repository");
    let mut repository = ConfigRepo::new(data_store);
    if with_remote {
        repository = repository.with_remote();
    }
    let repository = Arc::new(repository);
    let store = Arc::new(AgentControlConfigStore::new(repository.clone()));
    (repository, store)
}
