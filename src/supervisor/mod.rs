use std::{sync::mpsc::Sender, thread::JoinHandle};

use crate::command::stream::OutputEvent; // FIXME related to streaming. Move to own trait to hide OutputEvent

mod context;
mod error;

pub(crate) mod handle;
pub(crate) mod runner;

pub trait Runner {
    type E: std::error::Error + Send + Sync;

    /// The run method will execute a supervisor (non-blocking)
    fn run(
        self,
        ctx: context::SupervisorContext,
        tx: Sender<OutputEvent>, // FIXME related to streaming. Move to own trait to hide OutputEvent
    ) -> JoinHandle<Vec<Result<(), Self::E>>>;
}

pub trait Handle {
    type E: std::error::Error + Send + Sync;
    type R: Runner;

    /// The stop method will stop the supervisor's execution
    fn stop(self) -> Result</* Self::R */ (), Self::E>;
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use super::{runner::SupervisorRunner, *};

    // How should this supervisor work?
    #[test]
    fn test_supervisors() {
        // Create the common context
        let ctx = context::SupervisorContext::new();
        // Create streaming channel
        let (tx, rx) = std::sync::mpsc::channel();

        // Create 50 supervisors
        let agents: Vec<SupervisorRunner> = (0..50)
            .map(|_| SupervisorRunner::new("echo", vec!["hello!"]) /* TODO: I guess we could call `with_restart_policy()` here. */)
            .collect();

        // Run all the supervisors, getting the handles
        let handles = agents
            .into_iter()
            .map(|agent| agent.run(ctx.clone(), tx.clone()))
            .collect::<Vec<_>>();

        // Get any outputs
        thread::spawn(move || {
            rx.iter().for_each(|e| {
                println!("Received: {:?}", e);
            })
        });

        // Sleep for a while
        thread::sleep(Duration::from_secs(5));

        // Stop all the supervisors
        ctx.cancel_all().unwrap();

        // Wait for all the supervised processes to finish
        let results = handles.into_iter().map(|h| h.join().unwrap());

        // Check that all the processes have finished correctly
        assert_eq!(results.flatten().count(), 50);
    }
}
