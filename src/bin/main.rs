use std::process::ExitCode;

use meta_agent::agent::{error::AgentError, Agent};

fn main() -> ExitCode {
    match Agent::work() {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => exit_with_error(e),
    }
}

fn exit_with_error(e: AgentError) -> ExitCode {
    eprintln!("Error: {}", e);
    // Here we can control what exit code do we want to return based on the error type.
    // See the LoggingError case below for an example.
    match e {
        AgentError::Debug => ExitCode::SUCCESS,
        AgentError::ChannelExtractError => ExitCode::FAILURE,
        AgentError::LoggingError(_) => ExitCode::from(101),
        AgentError::ConfigResolveError(_) => ExitCode::FAILURE,
    }
}
