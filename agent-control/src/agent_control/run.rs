use super::defaults::{
    AGENT_CONTROL_DATA_DIR, AGENT_CONTROL_LOCAL_DATA_DIR, AGENT_CONTROL_LOG_DIR,
    DYNAMIC_AGENT_TYPE_DIR,
};
use crate::agent_control::config::AgentControlConfig;
use crate::agent_control::http_server::runner::Runner;
use crate::agent_type::embedded_registry::EmbeddedRegistry;
use crate::event::broadcaster::unbounded::UnboundedBroadcast;
use crate::event::{AgentControlEvent, ApplicationEvent, SubAgentEvent, channel::EventConsumer};
use crate::opamp::remote_config::validators::signature::validator::SignatureValidator;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::runtime::Runtime;

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
    /// the runner before the environment-specific store is available.
    /// Environment-specific `run()` methods re-load config from their
    /// respective stores (file / ConfigMap) to get the authoritative config.
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
    pub fn try_new(
        bootstrap_config: AgentControlConfig,
        base_paths: BasePaths,
        running_mode: Environment,
        application_event_consumer: EventConsumer<ApplicationEvent>,
    ) -> Result<Self, Box<dyn Error>> {
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?,
        );

        let mut agent_control_publisher = UnboundedBroadcast::default();
        let mut sub_agent_publisher = UnboundedBroadcast::default();
        let http_server_runner = bootstrap_config.server.enabled.then(|| {
            let agent_control_consumer = EventConsumer::from(agent_control_publisher.subscribe());
            let sub_agent_consumer = EventConsumer::from(sub_agent_publisher.subscribe());
            Runner::new(
                bootstrap_config.server.clone(),
                runtime.clone(),
                agent_control_consumer,
                sub_agent_consumer,
                bootstrap_config.fleet_control.clone(),
            )
        });

        let agent_type_registry = Arc::new(EmbeddedRegistry::new(
            base_paths.local_dir.join(DYNAMIC_AGENT_TYPE_DIR),
        ));

        let signature_validator = bootstrap_config
            .fleet_control
            .clone()
            .map(|fleet_config| {
                SignatureValidator::new(
                    fleet_config.signature_validation,
                    bootstrap_config.proxy.clone(),
                )
            })
            .transpose()?
            .unwrap_or(SignatureValidator::new_noop());

        Ok(AgentControlRunner {
            bootstrap_config,
            http_server_runner,
            runtime,
            agent_type_registry,
            application_event_consumer,
            agent_control_publisher,
            sub_agent_publisher,
            base_paths,
            signature_validator,
            running_mode,
        })
    }
}
