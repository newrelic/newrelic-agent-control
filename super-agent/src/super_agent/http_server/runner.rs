use crate::event::channel::EventConsumer;
use crate::event::SuperAgentEvent;
use crate::super_agent::http_server::async_bridge::run_async_sync_bridge;
use crate::super_agent::http_server::config::ServerConfig;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

/// Runner will be responsible for spawning the OS Thread for the HTTP Server
/// and owning the JoinHandle. It controls the server stop implementing drop
pub struct Runner {
    join_handle: Option<JoinHandle<()>>,
}

impl Runner {
    /// start the OS Thread with the HTTP Server and return a struct
    /// that holds the JoinHandle until drop
    pub fn start(
        config: ServerConfig,
        runtime: Arc<Runtime>,
        super_agent_consumer: EventConsumer<SuperAgentEvent>,
    ) -> Self {
        let join_handle = if config.enabled {
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
                let _ = runtime
                    .block_on(crate::super_agent::http_server::server::run_status_server(
                        config.clone(),
                        async_sa_event_consumer,
                    ))
                    .inspect_err(|err| {
                        error!(error_msg = err.to_string(), "error running status server");
                    });

                // Wait until the bridge is closed
                bridge_join_handle.join().unwrap();
            })
        } else {
            thread::spawn(move || loop {
                match super_agent_consumer.as_ref().recv() {
                    Ok(_) => {
                        //do nothing
                    }
                    Err(e) => {
                        debug!(
                            error_msg = e.to_string(),
                            "http server event drain processor closed"
                        );
                        break;
                    }
                }
            })
        };
        Runner {
            join_handle: Some(join_handle),
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        if let Some(join_handle) = self.join_handle.take() {
            info!("waiting for status server to stop gracefully...");
            join_handle
                .join()
                .expect("error waiting for server join handle")
        }
    }
}
