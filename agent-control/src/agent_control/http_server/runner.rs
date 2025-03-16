use crate::agent_control::config::OpAMPClientConfig;
use crate::agent_control::http_server::async_bridge::run_async_sync_bridge;
use crate::agent_control::http_server::config::ServerConfig;
use crate::event::channel::EventConsumer;
use crate::event::{AgentControlEvent, SubAgentEvent};
use crate::utils::threads::spawn_named_thread;
use crossbeam::select;
use std::sync::Arc;
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
    /// When the HTTP Server is disabled, it will spawn a thread
    /// with a consumer that will just consume events with no action
    /// to drain the channel and avoid memory leaks
    pub fn start(
        config: ServerConfig,
        runtime: Arc<Runtime>,
        agent_control_consumer: EventConsumer<AgentControlEvent>,
        sub_agent_consumer: EventConsumer<SubAgentEvent>,
        maybe_opamp_client_config: Option<OpAMPClientConfig>,
    ) -> Self {
        let join_handle = if config.enabled {
            Self::spawn_server(
                config,
                runtime,
                agent_control_consumer,
                sub_agent_consumer,
                maybe_opamp_client_config,
            )
        } else {
            // Spawn a thread with a no-action consumer to drain the channel and
            // avoid memory leaks
            Self::spawn_noop_consumer(agent_control_consumer, sub_agent_consumer)
        };
        Runner {
            join_handle: Some(join_handle),
        }
    }

    fn spawn_server(
        config: ServerConfig,
        runtime: Arc<Runtime>,
        agent_control_consumer: EventConsumer<AgentControlEvent>,
        sub_agent_consumer: EventConsumer<SubAgentEvent>,
        maybe_opamp_client_config: Option<OpAMPClientConfig>,
    ) -> JoinHandle<()> {
        spawn_named_thread("Http server", move || {
            // Create 2 unbounded channel to send the Agent Control and Sub Agent Sync events
            // to the Async Status Server
            let (async_agent_control_event_publisher, async_agent_control_event_consumer) =
                mpsc::unbounded_channel::<AgentControlEvent>();
            let (async_sub_agent_event_publisher, async_sub_agent_event_consumer) =
                mpsc::unbounded_channel::<SubAgentEvent>();
            // Run an OS Thread that listens to sync channel and forwards the events
            // to an async channel
            let bridge_join_handle = run_async_sync_bridge(
                async_agent_control_event_publisher,
                async_sub_agent_event_publisher,
                agent_control_consumer,
                sub_agent_consumer,
            );

            // Run the async status server
            let _ = runtime
                .block_on(
                    crate::agent_control::http_server::server::run_status_server(
                        config.clone(),
                        async_agent_control_event_consumer,
                        async_sub_agent_event_consumer,
                        maybe_opamp_client_config,
                    ),
                )
                .inspect_err(|err| {
                    error!(error_msg = %err, "error running status server");
                });

            // Wait until the bridge is closed
            bridge_join_handle.join().unwrap();
        })
    }

    fn spawn_noop_consumer(
        agent_control_consumer: EventConsumer<AgentControlEvent>,
        sub_agent_consumer: EventConsumer<SubAgentEvent>,
    ) -> JoinHandle<()> {
        spawn_named_thread("No-action consumer", move || loop {
            select! {
                recv(agent_control_consumer.as_ref()) -> agent_control_consumer_res => {
                    match agent_control_consumer_res {
                        Ok(_) => {}
                        Err(err) => {
                            debug!(
                                error_msg = %err,
                                "http server event drain processor closed"
                            );
                            break;
                        }
                    }
                },
                recv(sub_agent_consumer.as_ref()) -> sub_agent_consumer_res => {
                    match sub_agent_consumer_res {
                        Ok(_) => {}
                        Err(err) => {
                            debug!(
                                error_msg = %err,
                                "http server event drain processor closed"
                            );
                            break;
                        }
                    }
                }
            }
        })
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        if let Some(join_handle) = self.join_handle.take() {
            info!("waiting for status server to stop gracefully...");
            let _ = join_handle.join();
        }
    }
}
