use super::config::OpAMPClientConfig;
use super::defaults::{
    DYNAMIC_AGENT_TYPE_FILENAME, SUPER_AGENT_DATA_DIR, SUPER_AGENT_LOCAL_DATA_DIR,
    SUPER_AGENT_LOG_DIR,
};
use super::http_server::config::ServerConfig;
use crate::agent_type::embedded_registry::EmbeddedRegistry;
use crate::event::channel::pub_sub;
use crate::event::{
    channel::{EventConsumer, EventPublisher},
    ApplicationEvent, SubAgentEvent, SuperAgentEvent,
};
use crate::http::proxy::ProxyConfig;
use crate::opamp::auth::token_retriever::TokenRetrieverImpl;
use crate::opamp::http::builder::UreqHttpClientBuilder;
use crate::super_agent::http_server::runner::Runner;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tracing::{debug, error};

// k8s and on_host need to be public to allow integration tests to access the fn run_super_agent.
#[cfg(feature = "k8s")]
pub mod k8s;
#[cfg(feature = "onhost")]
pub mod on_host;

/// Structure with all base paths required to run the super agent
#[derive(Debug, Clone)]
pub struct BasePaths {
    pub local_dir: PathBuf,
    pub remote_dir: PathBuf,
    pub log_dir: PathBuf,
}

impl Default for BasePaths {
    fn default() -> Self {
        Self {
            local_dir: PathBuf::from(SUPER_AGENT_LOCAL_DATA_DIR),
            remote_dir: PathBuf::from(SUPER_AGENT_DATA_DIR),
            log_dir: PathBuf::from(SUPER_AGENT_LOG_DIR),
        }
    }
}

/// Structures for running the super-agent provided by CLI inputs
pub struct SuperAgentRunConfig {
    pub opamp: Option<OpAMPClientConfig>,
    pub http_server: ServerConfig,
    pub base_paths: BasePaths,
    pub proxy: ProxyConfig,
    #[cfg(feature = "k8s")]
    pub k8s_config: super::config::K8sConfig,
}

/// Structure with all the data required to run the super agent.
///
/// Fields are public just for testing. The object is destroyed right after is deleted,
/// Therefore, we should be worried of any tampering after its creation.
pub struct SuperAgentRunner {
    agent_type_registry: EmbeddedRegistry,
    application_event_consumer: EventConsumer<ApplicationEvent>,
    opamp_http_builder: Option<UreqHttpClientBuilder<TokenRetrieverImpl>>,
    super_agent_publisher: EventPublisher<SuperAgentEvent>,
    sub_agent_publisher: EventPublisher<SubAgentEvent>,
    base_paths: BasePaths,
    #[cfg(feature = "k8s")]
    k8s_config: super::config::K8sConfig,

    #[allow(dead_code)]
    runtime: Arc<Runtime>,

    // Since _http_server_runner drop depends on super_agent_publisher being drop before we need it
    // to be the last field of the struct. TODO Refactor runner so that this is not a risk anymore.
    _http_server_runner: Runner,
}

impl SuperAgentRunner {
    pub fn new(
        config: SuperAgentRunConfig,
        application_event_consumer: EventConsumer<ApplicationEvent>,
    ) -> Result<Self, Box<dyn Error>> {
        debug!("initializing and starting the super agent");

        let opamp_http_builder = match config.opamp.as_ref() {
            Some(opamp_config) => {
                debug!("OpAMP configuration found, creating an OpAMP client builder");

                let token_retriever = Arc::new(
                    TokenRetrieverImpl::try_build(
                        opamp_config.clone().auth_config,
                        config.base_paths.clone(),
                        config.proxy.clone(),
                    )
                    .inspect_err(|err| error!(error_mgs=%err,"Building token retriever"))?,
                );

                let http_builder =
                    UreqHttpClientBuilder::new(opamp_config.clone(), config.proxy, token_retriever);

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
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();
        let _http_server_runner = Runner::start(
            config.http_server,
            runtime.clone(),
            super_agent_consumer,
            sub_agent_consumer,
            config.opamp.clone(),
        );

        let agent_type_registry = EmbeddedRegistry::new(
            config
                .base_paths
                .local_dir
                .join(DYNAMIC_AGENT_TYPE_FILENAME),
        );

        Ok(SuperAgentRunner {
            _http_server_runner,
            runtime,
            #[cfg(feature = "k8s")]
            k8s_config: config.k8s_config,
            agent_type_registry,
            application_event_consumer,
            opamp_http_builder,
            super_agent_publisher,
            sub_agent_publisher,
            base_paths: config.base_paths,
        })
    }
}

#[cfg(debug_assertions)]
/// Set path override if local_dir, remote_dir, and logs_dir flags are set
pub fn set_debug_dirs(base_paths: BasePaths, cli: &crate::cli::Cli) -> BasePaths {
    let mut base_paths = base_paths;

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
