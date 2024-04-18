use crate::event::channel::EventConsumer;
use crate::event::SuperAgentEvent;
use crate::super_agent::http_server::async_bridge::run_async_sync_bridge;
use crate::super_agent::http_server::config::ServerConfig;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tracing::{error, info};

// Runner will be responsible for spawning the OS Thread for the HTTP Server
// and controlling the end of it (waiting until is finished using `drop`)
// Original idea was to use a single struct Runner and use a generic
// for the state Runner<NotStarted> and Runner<Started>. But with generics
// we cannot use drop and control the graceful shutdown. So it's divided in
// two structs.

// RunnerNotStarted is responsible for spawning the OS Thread with the HTTP Server and
// the bridge between sync/async events.
pub struct RunnerNotStarted {
    config: ServerConfig,
    runtime: Arc<Runtime>,
}

//RunnerStarted holds the join_handle of the OS Thread and waits for it on drop.
pub struct RunnerStarted {
    join_handle: Option<JoinHandle<()>>,
}

impl RunnerNotStarted {
    pub fn new(config: ServerConfig, runtime: Arc<Runtime>) -> Self {
        Self { config, runtime }
    }

    pub fn start(self, super_agent_consumer: EventConsumer<SuperAgentEvent>) -> RunnerStarted {
        let join_handle = self.config.enabled.then(|| {
            thread::spawn(move || {
                // Create unbounded channel to send the Super Agent Sync events
                // to the Async Status Server
                let (async_sa_event_publisher, async_sa_event_consumer) =
                    mpsc::unbounded_channel::<SuperAgentEvent>();
                // Run an OS Thread that listens to sync channel and forwards the events
                // to an async channel
                let bridge_join_handle =
                    run_async_sync_bridge(async_sa_event_publisher, super_agent_consumer);

                // Run the async status server
                let _ = self
                    .runtime
                    .block_on(crate::super_agent::http_server::server::run_status_server(
                        self.config.clone(),
                        async_sa_event_consumer,
                    ))
                    .inspect_err(|err| {
                        error!(error_msg = err.to_string(), "error running status server");
                    });

                // Wait until the bridge is closed
                bridge_join_handle.join().unwrap();
            })
        });

        RunnerStarted { join_handle }
    }
}

impl Drop for RunnerStarted {
    fn drop(&mut self) {
        if let Some(join_handle) = self.join_handle.take() {
            info!("waiting for status server to stop gracefully...");
            join_handle
                .join()
                .expect("error waiting for server join handle")
        }
    }
}
