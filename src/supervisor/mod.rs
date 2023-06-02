pub mod context;
mod error;
pub mod runner;

/// The Runner trait defines the entry-point interface for a supervisor. Exposes a run method that will start the supervised process' execution.
pub trait Runner {
    type E: std::error::Error + Send + Sync;
    type H: Handle;

    /// The run method will execute a supervisor (non-blocking). Returns a [`Handle`] to manage the running process.
    fn run(self) -> Self::H;
}

// TODO call this `into_inner` instead?
/// The Handle trait defines the interface for a supervised process' handle. It only exposes a getter for the inner handle.
pub trait Handle {
    type E: std::error::Error + Send + Sync;
    type S: Send + Sync;

    /// Return the inner handle of the supervised process.
    fn get_handle(self) -> Option<Self::S>;
}
