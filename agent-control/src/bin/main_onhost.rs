//! This is the entry point for the on-host implementation of Agent Control.
//!
//! It implements the basic functionality of parsing the command line arguments and either
//! performing one-shot actions or starting the main agent control process.
#![warn(missing_docs)]
use newrelic_agent_control::agent_control::run::AgentControlRunner;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use newrelic_agent_control::command::{Command, RunContext};
use newrelic_agent_control::utils::is_elevated::is_elevated;
use std::error::Error;
use std::process::ExitCode;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() -> ExitCode {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();
    #[cfg(feature = "dhat-ad-hoc")]
    let _profiler = dhat::Profiler::new_ad_hoc();

    #[cfg(target_family = "unix")]
    {
        Command::execute(AGENT_CONTROL_MODE_ON_HOST, _main)
    }

    #[cfg(target_os = "windows")]
    {
        use newrelic_agent_control::command::windows::WINDOWS_SERVICE_NAME;

        /// Entry-point for Windows Service
        fn service_main(_arguments: Vec<std::ffi::OsString>) {
            let _ = Command::execute(AGENT_CONTROL_MODE_ON_HOST, _main, true);
        }

        windows_service::define_windows_service!(ffi_service_main, service_main);

        if windows_service::service_dispatcher::start(WINDOWS_SERVICE_NAME, ffi_service_main)
            .is_err()
        {
            // Not running as Windows Service, run normally
            return Command::execute(AGENT_CONTROL_MODE_ON_HOST, _main, false);
        }
        ExitCode::SUCCESS
    }
}

/// This is the actual main function.
///
/// It is separated from [main] to allow propagating
/// the errors and log them in a string format, avoiding logging the error message twice.
/// If we just propagate the error to the main function, the error is logged in string format and
/// in "Rust mode", i.e. like this:
/// ```sh
/// could not read Agent Control config from /invalid/path: error loading the agent control config: \`error retrieving config: \`missing field \`agents\`\`\`
/// Error: ConfigRead(LoadConfigError(ConfigError(missing field \`agents\`)))
/// ```
fn _main(run_context: RunContext) -> Result<(), Box<dyn Error>> {
    #[cfg(not(feature = "disable-asroot"))]
    if !is_elevated()? {
        return Err("Program must run with elevated permissions".into());
    }

    #[cfg(all(target_family = "unix", not(feature = "multiple-instances")))]
    if let Err(err) = newrelic_agent_control::agent_control::pid_cache::PIDCache::from_data_dir(
        &run_context.run_config.base_paths.remote_dir,
    )
    .store(std::process::id())
    {
        return Err(format!("Error saving main process id: {err}").into());
    }

    // Create the actual agent control runner with the rest of required configs
    // and the application_event_consumer and capture the result to report the error in windows
    let run_result = AgentControlRunner::new(
        run_context.run_config,
        run_context.application_event_consumer,
    )
    .and_then(|runner| runner.run().map_err(|e| e.into()));

    #[cfg(target_family = "windows")]
    if let Some(handler) = run_context.stop_handler {
        // Teardown notifies Windows that we're stopping intentionally, avoiding a 1061 state.
        // 1061 occurs in Windows when a service is busy, unresponsive, or experiencing a conflict,
        // preventing it from starting, stopping, or restarting.
        if let Err(e) = handler.teardown(&run_result) {
            tracing::error!("Failed to report service stop to Windows: {e}");
        }
    }

    run_result
}
