use super::defaults::{
    AGENT_CONTROL_DATA_DIR, AGENT_CONTROL_LOCAL_DATA_DIR, AGENT_CONTROL_LOG_DIR,
    DYNAMIC_AGENT_TYPE_DIR,
};
use crate::agent_control::config::AgentControlConfig;
use crate::agent_control::config_repository::store::AgentControlConfigStore;
use crate::agent_control::http_server::runner::Runner;
use crate::agent_type::embedded_registry::EmbeddedRegistry;
use crate::command::RunnerContext;
use crate::data_store::DataStore;
use crate::event::broadcaster::unbounded::UnboundedBroadcast;
use crate::event::{AgentControlEvent, ApplicationEvent, SubAgentEvent, channel::EventConsumer};
use crate::opamp::remote_config::validators::signature::validator::SignatureValidator;
use crate::values::ConfigRepo;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tracing::debug;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct RunError(String);

// k8s and on_host need to be public to allow integration tests to access the fn run_agent_control.

pub mod k8s;
pub mod on_host;
pub mod runtime;

/// Defines the supported deployments for agent types
#[derive(Debug, PartialEq, Copy, Clone)]
pub enum Environment {
    Linux,
    Windows,
    K8s,
}

impl Display for Environment {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Environment::Linux => write!(f, "linux"),
            Environment::Windows => write!(f, "windows"),
            Environment::K8s => write!(f, "k8s"),
        }
    }
}

/// Structure with all base paths required to run Agent Control
#[derive(Debug, Clone)]
pub struct BasePaths {
    pub local_dir: PathBuf,
    pub remote_dir: PathBuf,
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

    agent_type_registry: Arc<EmbeddedRegistry>,
    application_event_consumer: EventConsumer<ApplicationEvent>,
    agent_control_publisher: UnboundedBroadcast<AgentControlEvent>,
    sub_agent_publisher: UnboundedBroadcast<SubAgentEvent>,
    signature_validator: SignatureValidator,
    base_paths: BasePaths,
    runtime: Arc<Runtime>,
    running_mode: Environment,
    http_server_runner: Option<Runner>,
}

impl AgentControlRunner {
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

        let agent_type_registry = Arc::new(EmbeddedRegistry::new(
            context.base_paths.local_dir.join(DYNAMIC_AGENT_TYPE_DIR),
        ));

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
            application_event_consumer: context.application_event_consumer,
            agent_control_publisher,
            sub_agent_publisher,
            base_paths: context.base_paths,
            signature_validator,
            running_mode: context.running_mode,
        })
    }
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
