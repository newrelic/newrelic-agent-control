use super::config::OpAMPClientConfig;
use super::defaults::{
    AGENT_CONTROL_DATA_DIR, AGENT_CONTROL_LOCAL_DATA_DIR, AGENT_CONTROL_LOG_DIR,
    DYNAMIC_AGENT_TYPE_FILENAME,
};
use super::http_server::config::ServerConfig;
use crate::agent_control::http_server::runner::Runner;
use crate::agent_type::embedded_registry::EmbeddedRegistry;
use crate::event::channel::pub_sub;
use crate::event::{
    channel::{EventConsumer, EventPublisher},
    AgentControlEvent, ApplicationEvent, SubAgentEvent,
};
use crate::http::config::ProxyConfig;
use crate::opamp::auth::token_retriever::TokenRetrieverImpl;
use crate::opamp::http::builder::OpAMPHttpClientBuilder;
use crate::opamp::remote_config::validators::signature::validator::{
    build_signature_validator, SignatureValidator,
};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use tracing::{debug, error};

// k8s and on_host need to be public to allow integration tests to access the fn run_agent_control.

pub mod k8s;

pub mod on_host;

/// Defines the supported deployments for agent types
#[derive(Debug, PartialEq, Clone)]
pub enum Environment {
    OnHost,
    K8s,
}

impl Display for Environment {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Environment::OnHost => write!(f, "on_host"),
            Environment::K8s => write!(f, "k8s"),
        }
    }
}

/// Structure with all base paths required to run the agent control
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

/// Structures for running the agent-control provided by CLI inputs
pub struct AgentControlRunConfig {
    pub opamp: Option<OpAMPClientConfig>,
    pub opamp_poll_interval: Duration,
    pub http_server: ServerConfig,
    pub base_paths: BasePaths,
    pub proxy: ProxyConfig,

    pub k8s_config: super::config::K8sConfig,

    pub garbage_collector_interval: Duration,
}

/// Structure with all the data required to run the agent control.
///
/// Fields are public just for testing. The object is destroyed right after is deleted,
/// Therefore, we should be worried of any tampering after its creation.
pub struct AgentControlRunner {
    agent_type_registry: Arc<EmbeddedRegistry>,
    application_event_consumer: EventConsumer<ApplicationEvent>,
    opamp_http_builder: Option<OpAMPHttpClientBuilder<TokenRetrieverImpl>>,
    opamp_poll_interval: Duration,
    agent_control_publisher: EventPublisher<AgentControlEvent>,
    sub_agent_publisher: EventPublisher<SubAgentEvent>,
    signature_validator: SignatureValidator,
    #[allow(dead_code, reason = "used by onhost")]
    base_paths: BasePaths,

    k8s_config: super::config::K8sConfig,

    garbage_collector_interval: Duration,

    #[allow(dead_code)]
    runtime: Arc<Runtime>,

    // Since _http_server_runner drop depends on agent_control_publisher being drop before we need it
    // to be the last field of the struct. TODO Refactor runner so that this is not a risk anymore.
    _http_server_runner: Runner,
}

impl AgentControlRunner {
    pub fn new(
        config: AgentControlRunConfig,
        application_event_consumer: EventConsumer<ApplicationEvent>,
    ) -> Result<Self, Box<dyn Error>> {
        debug!("initializing and starting the agent control");

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

                let http_builder = OpAMPHttpClientBuilder::new(
                    opamp_config.clone(),
                    config.proxy.clone(),
                    token_retriever,
                );

                Some(http_builder)
            }
            None => None,
        };
        let (agent_control_publisher, agent_control_consumer) = pub_sub::<AgentControlEvent>();
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?,
        );
        let (sub_agent_publisher, sub_agent_consumer) = pub_sub();
        let _http_server_runner = Runner::start(
            config.http_server,
            runtime.clone(),
            agent_control_consumer,
            sub_agent_consumer,
            config.opamp.clone(),
        );

        let agent_type_registry = Arc::new(EmbeddedRegistry::new(
            config
                .base_paths
                .local_dir
                .join(DYNAMIC_AGENT_TYPE_FILENAME),
        ));

        let signature_validator = config
            .opamp
            .map(|fleet_config| {
                build_signature_validator(fleet_config.signature_validation, config.proxy)
            })
            .transpose()?
            .unwrap_or(SignatureValidator::Noop);

        Ok(AgentControlRunner {
            _http_server_runner,
            runtime,

            k8s_config: config.k8s_config,

            garbage_collector_interval: config.garbage_collector_interval,
            agent_type_registry,
            application_event_consumer,
            opamp_http_builder,
            opamp_poll_interval: config.opamp_poll_interval,
            agent_control_publisher,
            sub_agent_publisher,
            base_paths: config.base_paths,
            signature_validator,
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
