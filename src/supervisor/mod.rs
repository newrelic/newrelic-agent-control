use std::thread::JoinHandle;

pub mod context;
mod error;
pub mod runner;

pub trait Runner {
    type E: std::error::Error + Send + Sync;
    type H: Handle;

    /// The run method will execute a supervisor (non-blocking)
    fn run(self) -> Self::H;
}

pub trait Handle {
    type E: std::error::Error + Send + Sync;

    /// The stop method will stop the supervisor's execution
    fn get_handles(self) -> JoinHandle<Result<(), Self::E>>;
}
