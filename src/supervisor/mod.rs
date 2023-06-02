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
            .map(
                |_| {
                    SupervisorRunner::new(
                        "echo".to_owned(),
                        vec!["hello!".to_owned()],
                        ctx.clone(),
                        tx.clone(),
                    )
                }, /* TODO: I guess we could call `with_restart_policy()` here. */
            )
            .collect();

        // Run all the supervisors, getting handles
        let handles = agents
            .into_iter()
            .map(|agent| agent.run())
            .collect::<Vec<_>>();

        // Get any outputs
        thread::spawn(move || {
            rx.iter().for_each(|e| {
                println!("Received: {:?}", e);
            })
        });

        // Sleep for a while
        thread::sleep(Duration::from_secs(1));

        // Stop all the supervisors
        ctx.cancel_all().unwrap();

        // Wait for all the supervised processes to finish
        let results = handles.into_iter().map(|h| h.get_handles().join().unwrap());

        // Check that all the processes have finished correctly
        assert_eq!(results.flatten().count(), 50);
    }
}
