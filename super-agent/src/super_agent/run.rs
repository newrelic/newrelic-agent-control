use super::config::OpAMPClientConfig;
use super::config_storer::store::SuperAgentConfigStore;
use super::http_server::config::ServerConfig;
use crate::event::channel::pub_sub;
use crate::opamp::auth::token_retriever::TokenRetrieverImpl;
use crate::opamp::http::builder::UreqHttpClientBuilder;
use crate::super_agent::http_server::runner::Runner;
use crate::{
    event::{
        channel::{EventConsumer, EventPublisher},
        ApplicationEvent, SuperAgentEvent,
    },
    opamp::client_builder::DefaultOpAMPClientBuilder,
};
#[cfg(feature = "k8s")]
use k8s::run_super_agent;
#[cfg(feature = "onhost")]
use on_host::run_super_agent;
use std::error::Error;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tracing::{debug, error, info, trace};

// k8s and on_host need to be public to allow integration tests to access the fn run_super_agent.
#[cfg(feature = "k8s")]
pub mod k8s;
#[cfg(feature = "onhost")]
pub mod on_host;

/// Structures for running the super-agent provided by CLI inputs
pub struct SuperAgentRunConfig {
    pub config_storer: SuperAgentConfigStore,
    pub opamp: Option<OpAMPClientConfig>,
    pub http_server: ServerConfig,
}

impl TryFrom<SuperAgentRunConfig> for SuperAgentRunner {
    type Error = Box<dyn Error>;

    fn try_from(value: SuperAgentRunConfig) -> Result<Self, Self::Error> {
        debug!("initializing and starting the super agent");

        trace!("creating the global context");
        let (application_event_publisher, application_event_consumer) = pub_sub();

        trace!("creating the signal handler");
        create_shutdown_signal_handler(application_event_publisher)?;

        let opamp_client_builder = match value.opamp.as_ref() {
            Some(opamp_config) => {
                debug!("OpAMP configuration found, creating an OpAMP client builder");
                let token_retriever = Arc::new(
                    TokenRetrieverImpl::try_from(opamp_config.clone())
                        .inspect_err(|err| error!(error_mgs=%err,"Building token retriever"))?,
                );

                let http_builder =
                    UreqHttpClientBuilder::new(opamp_config.clone(), token_retriever);
                Some(DefaultOpAMPClientBuilder::new(
                    opamp_config.clone(),
                    http_builder,
                ))
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
            opamp_client_builder,
            super_agent_publisher,
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
    opamp_client_builder:
        Option<DefaultOpAMPClientBuilder<UreqHttpClientBuilder<TokenRetrieverImpl>>>,
    super_agent_publisher: EventPublisher<SuperAgentEvent>,
}

/// Run the super agent with the provided data
impl SuperAgentRunner {
    pub fn run(self) -> Result<(), Box<dyn Error>> {
        Ok(run_super_agent(
            self.runtime.clone(),
            self.config_storer,
            self.application_event_consumer,
            self.opamp_client_builder,
            self.super_agent_publisher,
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
pub fn set_debug_dirs(cli: &crate::cli::Cli) {
    use crate::super_agent::defaults;

    if let Some(ref local_path) = cli.local_dir {
        defaults::set_local_dir(local_path);
    }
    if let Some(ref remote_path) = cli.remote_dir {
        defaults::set_remote_dir(remote_path);
    }
    if let Some(ref log_path) = cli.logs_dir {
        defaults::set_log_dir(log_path);
    }
    if let Some(ref debug_path) = cli.debug {
        defaults::set_debug_mode_dirs(debug_path);
    }
}
