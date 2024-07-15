use super::config::OpAMPClientConfig;
use super::config_storer::store::SuperAgentConfigStore;
use super::defaults::{
    DYNAMIC_AGENT_TYPE_FILENAME, SUPER_AGENT_DATA_DIR, SUPER_AGENT_LOCAL_DATA_DIR,
    SUPER_AGENT_LOG_DIR,
};
use super::http_server::config::ServerConfig;
use crate::agent_type::embedded_registry::EmbeddedRegistry;
use crate::event::channel::pub_sub;
use crate::event::{
    channel::{EventConsumer, EventPublisher},
    ApplicationEvent, SuperAgentEvent,
};
use crate::opamp::auth::token_retriever::TokenRetrieverImpl;
use crate::opamp::http::builder::UreqHttpClientBuilder;
use crate::super_agent::http_server::runner::Runner;
#[cfg(feature = "k8s")]
use k8s::run_super_agent;
#[cfg(feature = "onhost")]
use on_host::run_super_agent;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tracing::{debug, error, info, trace};

// k8s and on_host need to be public to allow integration tests to access the fn run_super_agent.
#[cfg(feature = "k8s")]
pub mod k8s;
#[cfg(feature = "onhost")]
pub mod on_host;

/// Structure with all base paths required to run the super agent
#[derive(Clone)]
pub struct BasePaths {
    pub local_dir: PathBuf,
    pub remote_dir: PathBuf,
    pub log_dir: PathBuf,
}

impl Default for BasePaths {
    fn default() -> Self {
        Self {
            local_dir: PathBuf::from(SUPER_AGENT_LOCAL_DATA_DIR()),
            remote_dir: PathBuf::from(SUPER_AGENT_DATA_DIR()),
            log_dir: PathBuf::from(SUPER_AGENT_LOG_DIR()),
        }
    }
}

/// Structures for running the super-agent provided by CLI inputs
pub struct SuperAgentRunConfig {
    pub config_storer: SuperAgentConfigStore,
    pub opamp: Option<OpAMPClientConfig>,
    pub http_server: ServerConfig,
    pub base_paths: BasePaths,
}

impl TryFrom<SuperAgentRunConfig> for SuperAgentRunner {
    type Error = Box<dyn Error>;

    fn try_from(value: SuperAgentRunConfig) -> Result<Self, Self::Error> {
        debug!("initializing and starting the super agent");

        trace!("creating the global context");
        let (application_event_publisher, application_event_consumer) = pub_sub();

        trace!("creating the signal handler");
        create_shutdown_signal_handler(application_event_publisher)?;

        let opamp_http_builder = match value.opamp.as_ref() {
            Some(opamp_config) => {
                debug!("OpAMP configuration found, creating an OpAMP client builder");
                let token_retriever = Arc::new(
                    TokenRetrieverImpl::try_from(opamp_config.clone())
                        .inspect_err(|err| error!(error_mgs=%err,"Building token retriever"))?,
                );

                let http_builder =
                    UreqHttpClientBuilder::new(opamp_config.clone(), token_retriever);

                Some(http_builder)
            }
            None => None,
        };
        let (super_agent_publisher, super_agent_consumer) = pub_sub::<SuperAgentEvent>();
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?,
        );
        let _started_http_server_runner = Runner::start(
            value.http_server,
            runtime.clone(),
            super_agent_consumer,
            value.opamp.clone(),
        );

        let run_data = SuperAgentRunner {
            _http_server_runner: _started_http_server_runner,
            runtime,
            config_storer: value.config_storer,
            application_event_consumer,
            opamp_http_builder,
            super_agent_publisher,
            base_paths: value.base_paths,
        };

        Ok(run_data)
    }
}

/// Structure with all the data required to run the super agent
// TODO: Generalize over injected dependencies like UreqHttpClientBuilder and TokenRetrieverImpl?
pub struct SuperAgentRunner {
    _http_server_runner: Runner,
    runtime: Arc<Runtime>,
    config_storer: SuperAgentConfigStore,
    application_event_consumer: EventConsumer<ApplicationEvent>,
    opamp_http_builder: Option<UreqHttpClientBuilder<TokenRetrieverImpl>>,
    super_agent_publisher: EventPublisher<SuperAgentEvent>,
    base_paths: BasePaths,
}

/// Run the super agent with the provided data
impl SuperAgentRunner {
    pub fn run(self) -> Result<(), Box<dyn Error>> {
        let agent_type_registry = EmbeddedRegistry::new(
            self.base_paths
                .local_dir
                .join(DYNAMIC_AGENT_TYPE_FILENAME()),
        );
        Ok(run_super_agent(
            self.runtime.clone(),
            self.config_storer,
            self.application_event_consumer,
            self.opamp_http_builder,
            self.super_agent_publisher,
            agent_type_registry,
            self.base_paths,
        )?)
    }
}

pub fn create_shutdown_signal_handler(
    publisher: EventPublisher<ApplicationEvent>,
) -> Result<(), ctrlc::Error> {
    ctrlc::set_handler(move || {
        info!("Received SIGINT (Ctrl-C). Stopping super agent");
        let _ = publisher
            .publish(ApplicationEvent::StopRequested)
            .map_err(|_| error!("Could not send super agent stop request"));
    })
    .map_err(|e| {
        error!("Could not set signal handler: {}", e);
        e
    })?;

    Ok(())
}

#[cfg(debug_assertions)]
/// Set the debug directories if the debug, or any of path override flags are set.
/// Precedence is given to the individual local_dir, remote_dir, and logs_dir flags
/// then the debug flag.
pub fn set_debug_dirs(base_paths: BasePaths, cli: &crate::cli::Cli) -> BasePaths {
    let mut base_paths = base_paths;

    if let Some(ref debug_path) = cli.debug {
        base_paths.local_dir = debug_path.join("nrsa_local");
        base_paths.remote_dir = debug_path.join("nrsa_remote");
        base_paths.log_dir = debug_path.join("nrsa_logs");
    }

    if let Some(ref local_path) = cli.local_dir {
        base_paths.local_dir = local_path.to_path_buf();
    }
    if let Some(ref remote_path) = cli.remote_dir {
        base_paths.remote_dir = remote_path.to_path_buf();
    }
    if let Some(ref log_path) = cli.logs_dir {
        base_paths.log_dir = log_path.to_path_buf();
    }

    base_paths
}
