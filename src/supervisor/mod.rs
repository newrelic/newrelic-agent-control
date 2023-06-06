pub mod context;
mod error;
pub mod newrelic_infra;
pub mod runner;

/// The Runner trait defines the entry-point interface for a supervisor. Exposes a run method that will start the supervised process' execution.
pub trait Runner {
    type E: std::error::Error + Send + Sync;
    type H: Handle;

    /// The run method will execute a supervisor (non-blocking). Returns a [`Handle`] to manage the running process.
    fn run(self) -> Self::H;
}

/// The Handle trait defines the interface for a supervised process' handle. Exposes a stop method that will cancel the supervised process' execution.
pub trait Handle {
    type E: std::error::Error + Send + Sync;
    type S: Send + Sync;

    /// Cancels the supervised process and returns its inner handle.
    fn stop(self) -> Self::S;
}
