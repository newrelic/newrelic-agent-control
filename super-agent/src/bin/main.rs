use newrelic_super_agent::cli::Cli;
use newrelic_super_agent::event::channel::pub_sub;
use newrelic_super_agent::event::SuperAgentEvent;
use newrelic_super_agent::opamp::auth::token_retriever::TokenRetrieverImpl;
use newrelic_super_agent::opamp::client_builder::DefaultOpAMPClientBuilder;
use newrelic_super_agent::opamp::http::builder::UreqHttpClientBuilder;
use newrelic_super_agent::super_agent::config_storer::loader_storer::SuperAgentConfigLoader;
use newrelic_super_agent::super_agent::config_storer::store::SuperAgentConfigStore;
#[cfg(debug_assertions)]
use newrelic_super_agent::super_agent::defaults;
use newrelic_super_agent::super_agent::http_server::runner::Runner;
use newrelic_super_agent::super_agent::run::{create_shutdown_signal_handler, run_super_agent};
use newrelic_super_agent::utils::binary_metadata::binary_metadata;
use std::error::Error;
use std::sync::Arc;
use tracing::{debug, error, info};

#[cfg(all(feature = "onhost", feature = "k8s", not(feature = "ci")))]
compile_error!("Feature \"onhost\" and feature \"k8s\" cannot be enabled at the same time");

#[cfg(all(not(feature = "onhost"), not(feature = "k8s")))]
compile_error!("Either feature \"onhost\" or feature \"k8s\" must be enabled");

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::init_super_agent_cli();

    if cli.print_version() {
        println!("{}", binary_metadata());
        return Ok(());
    }

    #[cfg(debug_assertions)]
    {
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

    let sa_local_config_storer = SuperAgentConfigStore::new(&cli.get_config_path());

    let super_agent_config = sa_local_config_storer.load().inspect_err(|err| {
        println!(
            "Could not read Super Agent config from {}: {}",
            sa_local_config_storer.config_path().to_string_lossy(),
            err
        )
    })?;

    // init logging singleton
    // If file logging is enabled, this will return a `WorkerGuard` value that needs to persist
    // as long as we want the logs to be written to file, hence, we assign it here so it is dropped
    // when the program exits.
    let _guard = super_agent_config.log.try_init()?;
    info!(
        "Starting NewRelic Super Agent with config '{}'",
        sa_local_config_storer.config_path().to_string_lossy()
    );

    info!("{}", binary_metadata());
    if cli.print_debug_info() {
        println!("Printing debug info");
        println!("CLI: {:#?}", cli);

        #[cfg(feature = "onhost")]
        println!("Feature: onhost");
        #[cfg(feature = "k8s")]
        println!("Feature: k8s");

        return Ok(());
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
        super_agent_config.server.clone(),
        runtime.clone(),
        super_agent_consumer,
        super_agent_config.opamp.clone(),
    );

    run_super_agent(
        runtime.clone(),
        sa_local_config_storer,
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
