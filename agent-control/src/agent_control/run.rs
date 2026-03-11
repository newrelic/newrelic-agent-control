use super::config::{K8sConfig, OpAMPClientConfig};
use super::defaults::{
    AGENT_CONTROL_DATA_DIR, AGENT_CONTROL_LOCAL_DATA_DIR, AGENT_CONTROL_LOG_DIR,
    DYNAMIC_AGENT_TYPE_DIR,
};
use super::http_server::config::ServerConfig;
use crate::agent_control::http_server::runner::Runner;
use crate::agent_type::embedded_registry::EmbeddedRegistry;
use crate::agent_type::variable::constraints::VariableConstraints;
use crate::event::broadcaster::unbounded::UnboundedBroadcast;
use crate::event::{AgentControlEvent, ApplicationEvent, SubAgentEvent, channel::EventConsumer};
use crate::http::config::ProxyConfig;
use crate::opamp::auth::token_retriever::TokenRetrieverImpl;
use crate::opamp::client_builder::DefaultOpAMPClientBuilder;
use crate::opamp::effective_config::loader::DefaultEffectiveConfigLoaderBuilder;
use crate::opamp::http::builder::OpAMPHttpClientBuilder;
use crate::opamp::remote_config::validators::signature::validator::SignatureValidator;
use crate::secret_retriever::OpampSecretRetriever;
use crate::values::config_repository::ConfigRepository;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tracing::{debug, error, info};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct RunError(String);

// k8s and on_host need to be public to allow integration tests to access the fn run_agent_control.

pub mod k8s;
pub mod on_host;
pub mod runtime;

/// OpAMPClientBuilder type alias for the builder used when building opamp clients.
type OpampClientBuilder<Y> = DefaultOpAMPClientBuilder<
    OpAMPHttpClientBuilder<TokenRetrieverImpl>,
    DefaultEffectiveConfigLoaderBuilder<Y>,
>;

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

/// Structures for running Agent Control provided by CLI inputs
pub struct AgentControlRunConfig {
    pub opamp: Option<OpAMPClientConfig>,
    pub http_server: ServerConfig,
    pub base_paths: BasePaths,
    pub proxy: ProxyConfig,
    pub k8s_config: K8sConfig,
    pub agent_type_var_constraints: VariableConstraints,
    pub ac_running_mode: Environment,
}

/// Structure with all the data required to run the agent control.
///
/// Fields are public just for testing. The object is destroyed right after is deleted,
/// Therefore, we should be worried of any tampering after its creation.
pub struct AgentControlRunner {
    agent_type_registry: Arc<EmbeddedRegistry>,
    application_event_consumer: EventConsumer<ApplicationEvent>,
    agent_control_publisher: UnboundedBroadcast<AgentControlEvent>,
    sub_agent_publisher: UnboundedBroadcast<SubAgentEvent>,
    signature_validator: SignatureValidator,
    base_paths: BasePaths,
    k8s_config: K8sConfig,
    runtime: Arc<Runtime>,
    ac_running_mode: Environment,
    http_server_runner: Option<Runner>,
    agent_type_var_constraints: VariableConstraints,
    proxy: ProxyConfig,
    opamp: Option<OpAMPClientConfig>,
}

impl AgentControlRunner {
    pub fn new(
        config: AgentControlRunConfig,
        application_event_consumer: EventConsumer<ApplicationEvent>,
    ) -> Result<Self, Box<dyn Error>> {
        debug!("initializing and starting the agent control");

        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?,
        );

        let mut agent_control_publisher = UnboundedBroadcast::default();
        let mut sub_agent_publisher = UnboundedBroadcast::default();
        let http_server_runner = config.http_server.enabled.then(|| {
            let agent_control_consumer = EventConsumer::from(agent_control_publisher.subscribe());
            let sub_agent_consumer = EventConsumer::from(sub_agent_publisher.subscribe());
            Runner::new(
                config.http_server,
                runtime.clone(),
                agent_control_consumer,
                sub_agent_consumer,
                config.opamp.clone(),
            )
        });

        let agent_type_registry = Arc::new(EmbeddedRegistry::new(
            config.base_paths.local_dir.join(DYNAMIC_AGENT_TYPE_DIR),
        ));

        let signature_validator = config
            .opamp
            .clone()
            .map(|fleet_config| {
                SignatureValidator::new(fleet_config.signature_validation, config.proxy.clone())
            })
            .transpose()?
            .unwrap_or(SignatureValidator::new_noop());

        Ok(AgentControlRunner {
            http_server_runner,
            runtime,
            k8s_config: config.k8s_config,
            agent_type_registry,
            application_event_consumer,
            agent_control_publisher,
            sub_agent_publisher,
            base_paths: config.base_paths,
            signature_validator,
            ac_running_mode: config.ac_running_mode,
            agent_type_var_constraints: config.agent_type_var_constraints,
            proxy: config.proxy,
            opamp: config.opamp,
        })
    }

    pub fn run(self) -> Result<(), RunError> {
        let run_result = match self.ac_running_mode {
            Environment::Linux | Environment::Windows => self.run_onhost(),
            Environment::K8s => self.run_k8s(),
        };

        run_result
            .inspect_err(|e| error!("Agent Control Runner failed: {e}"))
            .inspect(|_| info!("Exiting gracefully"))
    }
}

/// Helper to return the OpAMPClientBuilder for any implementation of [OpampSecretRetriever] and [ConfigRepository]
pub fn opamp_client_builder<R, Y>(
    config: OpAMPClientConfig,
    proxy_config: ProxyConfig,
    secret_retriever: R,
    config_repository: Arc<Y>,
) -> Result<OpampClientBuilder<Y>, RunError>
where
    R: OpampSecretRetriever,
    Y: ConfigRepository,
{
    let private_key = secret_retriever
        .retrieve()
        .map_err(|e| RunError(format!("error trying to get secret or private key {e}")))?;

    let token_retriever = Arc::new(
        TokenRetrieverImpl::try_build(
            config.clone().auth_config,
            private_key,
            proxy_config.clone(),
        )
        .inspect_err(|err| error!("Could not build OpAMP's token retriever: {err}"))
        .map_err(|e| {
            RunError(format!(
                "error trying to build OpAMP's token retriever: {e}"
            ))
        })?,
    );

    let poll_interval = config.poll_interval;

    let http_client_builder = OpAMPHttpClientBuilder::new(config, proxy_config, token_retriever);

    let config_loader_builder = DefaultEffectiveConfigLoaderBuilder::new(config_repository);

    Ok(DefaultOpAMPClientBuilder::new(
        http_client_builder,
        config_loader_builder,
        poll_interval,
    ))
}

#[cfg(debug_assertions)]
/// Set path override if local_dir, remote_dir, and logs_dir flags are set
pub fn set_debug_dirs(base_paths: BasePaths, cli: &crate::command::Command) -> BasePaths {
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
