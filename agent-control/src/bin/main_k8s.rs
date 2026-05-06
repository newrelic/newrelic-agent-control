//! This is the entry point for the Kubernetes implementation of Agent Control.
//!
//! It implements the basic functionality of parsing the command line arguments and either
//! performing one-shot actions or starting the main agent control process.
#![warn(missing_docs)]

use newrelic_agent_control::agent_control::run::AgentControlRunner;
use newrelic_agent_control::agent_control::run::k8s::AGENT_CONTROL_MODE_K8S;
use newrelic_agent_control::command::{Command, Context};
#[cfg(feature = "dhat-heap")]
use newrelic_agent_control::command::{dhat_init, dhat_write};
use std::error::Error;
use std::process::ExitCode;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() -> ExitCode {
    #[cfg(feature = "dhat-heap")]
    println!("DHAT PROFILING ACTIVE");
    #[cfg(feature = "dhat-heap")]
    {
        let profiler_path = std::env::var("AC_PROFILING_PATH").unwrap();
        let profiler = dhat::Profiler::builder().file_name(profiler_path).build();
        dhat_init(profiler);
    }

    #[cfg(feature = "dhat-ad-hoc")]
    let _profiler = dhat::Profiler::new_ad_hoc();

    #[cfg(target_family = "unix")]
    let result = Command::execute(AGENT_CONTROL_MODE_K8S, _main);
    #[cfg(target_family = "windows")]
    let result = Command::execute(AGENT_CONTROL_MODE_K8S, _main, false);

    // Fallback: write profile if SIGTERM handler didn't already (e.g. clean exit or SIGINT).
    #[cfg(feature = "dhat-heap")]
    dhat_write();

    result
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
fn _main(context: Context) -> Result<(), Box<dyn Error>> {
    // Create the actual agent control runner with the rest of required configs and the application_event_consumer
    AgentControlRunner::try_new(context.ac_runner_context)?
        .run_k8s()
        .map_err(|e| e.into())
        .map(|_shutdown_reason| ())
}
