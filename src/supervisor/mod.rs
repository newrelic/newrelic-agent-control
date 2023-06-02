use std::thread::JoinHandle;

mod context;
mod error;

pub(crate) mod handle;
pub(crate) mod runner;

pub trait Runner {
    type E: std::error::Error + Send + Sync;

    /// The run method will execute a supervisor (non-blocking)
    fn run(&mut self) -> JoinHandle<Result<(), Self::E>>;
}

pub trait Handle {
    type E: std::error::Error + Send + Sync;

    /// The stop method will stop the supervisor's execution
    fn stop(self) -> Result<(), Self::E>;
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

        // Run all the supervisors, getting the handles)
        let agents_handles = agents
            .into_iter()
            .map(|mut agent| {
                let handle = agent.run();
                (agent, handle)
            })
            .collect::<Vec<_>>();

        // Get any outputs
        thread::spawn(move || {
            rx.iter().for_each(|e| {
                println!("Received: {:?}", e);
            })
        });

        // Sleep for a while
        thread::sleep(Duration::from_secs(1));

        let (agents, handles) = agents_handles.into_iter().unzip::<_, _, Vec<_>, Vec<_>>();

        // Stop all the supervisors
        let _stopped = agents.into_iter().map(|a| a.stop()).collect::<Vec<_>>();

        // Wait for all the supervised processes to finish
        let results = handles.into_iter().map(|h| h.join().unwrap());

        // Check that all the processes have finished correctly
        assert_eq!(results.flatten().count(), 50);
    }
}

