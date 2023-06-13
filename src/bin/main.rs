use meta_agent::agent::Agent;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Ok(Agent::work()?)
}

/*
We can also make use of `main() -> ExitCode` if we want to return more specific exit codes
instead of just success or failure states (What returning `Result` does):

fn main() -> ExitCode {
    // Assumi8ng Agent::work() returns an `i32`...
    ExitCode::from(Agent::work())
    // We could also check the actual errors here and decide what specific exit code to return
    // based on the error type. Something like:
    match Agent::work() {
        Ok(_) => ExitCode::SUCCESS,
        Err(AgentError::ConfigNotFound) => ExitCode::from(101),
        Err(AgentError::ConfigParseError) => ExitCode::from(102),
        // ...
    }
}
 */
