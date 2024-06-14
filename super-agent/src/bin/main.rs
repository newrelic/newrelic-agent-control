use newrelic_super_agent::cli::{Cli, CliCommand};
use newrelic_super_agent::event::channel::pub_sub;
use newrelic_super_agent::event::SuperAgentEvent;
use newrelic_super_agent::logging::config::FileLoggerGuard;
use newrelic_super_agent::opamp::auth::token_retriever::TokenRetrieverImpl;
use newrelic_super_agent::opamp::client_builder::DefaultOpAMPClientBuilder;
use newrelic_super_agent::opamp::http::builder::UreqHttpClientBuilder;
use newrelic_super_agent::super_agent::http_server::runner::Runner;
use newrelic_super_agent::super_agent::run::{create_shutdown_signal_handler, run_super_agent};
use std::error::Error;
use std::sync::Arc;
use tracing::{debug, error, info};

#[cfg(all(feature = "onhost", feature = "k8s", not(feature = "ci")))]
compile_error!("Feature \"onhost\" and feature \"k8s\" cannot be enabled at the same time");

#[cfg(all(not(feature = "onhost"), not(feature = "k8s")))]
compile_error!("Either feature \"onhost\" or feature \"k8s\" must be enabled");

fn main() -> Result<(), Box<dyn Error>> {
    let super_agent_config = match Cli::init()? {
        // Super Agent command call instructs normal operation. Continue with required data.
        CliCommand::InitSuperAgent(cli) => cli,
        // Super Agent command call was an "one-shot" operation. Exit successfully.
        CliCommand::Quit => return Ok(()),
    };

    // Acquire the file logger guard (if any) for the whole duration of the program
    let _guard: FileLoggerGuard = super_agent_config.file_logger_guard;

    #[cfg(unix)]
    if !nix::unistd::Uid::effective().is_root() {
        error!("Program must run as root");
        std::process::exit(1);
    }

    debug!("Creating the global context");
    let (application_event_publisher, application_event_consumer) = pub_sub();

    debug!("Creating the signal handler");
    create_shutdown_signal_handler(application_event_publisher)?;

    let opamp_client_builder = match super_agent_config.opamp.as_ref() {
        Some(opamp_config) => {
            let token_retriever = Arc::new(
                TokenRetrieverImpl::try_from(opamp_config.clone())
                    .inspect_err(|err| error!(error_mgs=%err,"Building token retriever"))?,
            );

            let http_builder = UreqHttpClientBuilder::new(opamp_config.clone(), token_retriever);
            Some(DefaultOpAMPClientBuilder::new(
                opamp_config.clone(),
                http_builder,
            ))
        }
        None => None,
    };

    // create Super Agent events channel
    let (super_agent_publisher, super_agent_consumer) = pub_sub::<SuperAgentEvent>();

    // Create the Tokio runtime
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?,
    );

    // Start the status server if enabled. Stopping control is done through events
    // Dropping _started_runner will force it to wait until the server is gracefully
    // stopped
    let _started_http_server_runner = Runner::start(
        super_agent_config.http_server,
        runtime.clone(),
        super_agent_consumer,
        super_agent_config.opamp.clone(),
    );

    run_super_agent(
        runtime.clone(),
        super_agent_config.config_storer,
        application_event_consumer,
        opamp_client_builder,
        super_agent_publisher,
    )
    .inspect_err(|err| {
        error!(
            "The super agent main process exited with an error: {}",
            err.to_string()
        )
    })?;

    info!("exiting gracefully");
    Ok(())
}
