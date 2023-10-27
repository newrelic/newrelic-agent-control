pub mod collection;
pub mod error;
pub mod on_host;

// CRATE TRAITS

/// The Runner trait defines the entry-point interface for a supervisor. Exposes a run method that will start the supervised process' execution.
pub trait NotStartedSubAgent {
    type StartedSubAgent: StartedSubAgent;

    /// The run method will execute a supervisor (non-blocking). Returns a [`StartedSubAgent`] to manage the running process.
    fn run(self) -> Result<Self::StartedSubAgent, error::SubAgentError>;
}

/// The Handle trait defines the interface for a supervised process' handle. Exposes a stop method that will cancel the supervised process' execution.
pub trait StartedSubAgent {
    type S: Send + Sync;

    /// Cancels the supervised process and returns its inner handle.
    fn stop(self) -> Result<Vec<Self::S>, error::SubAgentError>;
}

pub trait SubAgentBuilder {
    type S: NotStartedSubAgent;
    fn build(&self) -> Result<Self::S, error::SubAgentBuilderError>;
}
