use crate::agent_control::config::OpAMPClientConfig;
use crate::agent_control::http_server::async_bridge::run_async_sync_bridge;
use crate::agent_control::http_server::config::ServerConfig;
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::EventConsumer;
use crate::event::{AgentControlEvent, SubAgentEvent};
use crate::utils::thread_context::{NotStartedThreadContext, StartedThreadContext};
use crossbeam::select;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

/// This struct holds the information required to start the HTTP Server and it is
/// responsible for starting it.
pub struct Runner {
    config: ServerConfig,
    runtime: Arc<Runtime>,
    agent_control_consumer: EventConsumer<AgentControlEvent>,
    sub_agent_consumer: EventConsumer<SubAgentEvent>,
    maybe_opamp_client_config: Option<OpAMPClientConfig>,
}

/// This struct is responsible for spawning the OS Thread for the HTTP Server
/// and owning the JoinHandle. It controls the server stop implementing drop
pub struct StartedHttpServer {
    thread_context: Option<StartedThreadContext>,
}

impl Runner {
    pub fn new(
        config: ServerConfig,
        runtime: Arc<Runtime>,
        agent_control_consumer: EventConsumer<AgentControlEvent>,
        sub_agent_consumer: EventConsumer<SubAgentEvent>,
        maybe_opamp_client_config: Option<OpAMPClientConfig>,
    ) -> Self {
        Self {
            config,
            runtime,
            agent_control_consumer,
            sub_agent_consumer,
            maybe_opamp_client_config,
        }
    }
    /// start the OS Thread with the HTTP Server and return a struct
    /// that holds the JoinHandle until drop
    /// When the HTTP Server is disabled, it will spawn a thread
    /// with a consumer that will just consume events with no action
    /// to drain the channel and avoid memory leaks
    pub fn start(self) -> StartedHttpServer {
        let thread_context = if self.config.enabled {
            let callback = move |stop_consumer: EventConsumer<CancellationMessage>| {
                self.spawn_server(stop_consumer)
            };
            NotStartedThreadContext::new("Http server", callback).start()
        } else {
            let callback = move |stop_consumer: EventConsumer<CancellationMessage>| {
                self.noop_consumer_loop(stop_consumer)
            };
            // Spawn a thread with a no-action consumer to drain the channel and
            // avoid memory leaks
            NotStartedThreadContext::new("No-action consumer", callback).start()
        };

        StartedHttpServer {
            thread_context: Some(thread_context),
        }
    }

    fn spawn_server(self, stop_rx: EventConsumer<CancellationMessage>) {
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
            self.agent_control_consumer,
            self.sub_agent_consumer,
            stop_rx,
        );

        // Run the async status server
        let _ = self
            .runtime
            .block_on(
                crate::agent_control::http_server::server::run_status_server(
                    self.config.clone(),
                    async_agent_control_event_consumer,
                    async_sub_agent_event_consumer,
                    self.maybe_opamp_client_config,
                ),
            )
            .inspect_err(|err| {
                error!(error_msg = %err, "error running status server");
            });

        // Wait until the bridge is closed
        bridge_join_handle.join().unwrap();
    }

    fn noop_consumer_loop(self, stop_rx: EventConsumer<CancellationMessage>) {
        loop {
            select! {
                recv(self.agent_control_consumer.as_ref()) -> agent_control_consumer_res => {
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
                recv(self.sub_agent_consumer.as_ref()) -> sub_agent_consumer_res => {
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
                },
                recv(stop_rx.as_ref()) -> _ => {
                    debug!("http server event drain processor stopped");
                    break;
                },
            }
        }
    }
}

impl Drop for StartedHttpServer {
    fn drop(&mut self) {
        info!("waiting for status server to stop gracefully...");

        let Some(thread_context) = self.thread_context.take() else {
            return;
        };

        let _ = thread_context
            .stop()
            .inspect(|_| debug!("status server runner thread stopped"))
            .inspect_err(|error_msg| error!("Error stopping Status Server: {error_msg}"));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;

    use tracing_test::internal::logs_with_scope_contain;
    use tracing_test::traced_test;

    use crate::agent_control::http_server::config::ServerConfig;
    use crate::event::AgentControlEvent;
    use crate::event::channel::pub_sub;

    use super::*;

    #[test]
    #[traced_test]
    fn test_noop_consumer_stops_gracefully_when_dropped() {
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap(),
        );
        let (_agent_control_publisher, agent_control_consumer) = pub_sub::<AgentControlEvent>();
        let (_sub_agent_publisher, sub_agent_consumer) = pub_sub();
        let _started_http_server = Runner::new(
            ServerConfig::default(),
            runtime,
            agent_control_consumer,
            sub_agent_consumer,
            None,
        )
        .start();
        drop(_started_http_server);

        // wait for logs to be flushed
        sleep(Duration::from_millis(100));
        assert!(logs_with_scope_contain(
            "newrelic_agent_control::agent_control::http_server::runner",
            "http server event drain processor stopped",
        ));
    }
    #[test]
    #[traced_test]
    fn test_server_stops_gracefully_when_dropped() {
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap(),
        );
        let (_agent_control_publisher, agent_control_consumer) = pub_sub::<AgentControlEvent>();
        let (_sub_agent_publisher, sub_agent_consumer) = pub_sub();
        let _started_http_server = Runner::new(
            ServerConfig {
                enabled: true,
                port: 0.into(),
                ..Default::default()
            },
            runtime,
            agent_control_consumer,
            sub_agent_consumer,
            None,
        )
        .start();
        // server warm up
        sleep(Duration::from_millis(100));

        drop(_started_http_server);

        // wait for logs to be flushed
        sleep(Duration::from_millis(100));
        assert!(logs_with_scope_contain(
            "newrelic_agent_control::agent_control::http_server::server",
            "status server gracefully stopped",
        ));
    }
    #[test]
    #[traced_test]
    fn test_server_stops_gracefully_when_external_channels_close() {
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap(),
        );
        let (_agent_control_publisher, agent_control_consumer) = pub_sub::<AgentControlEvent>();
        let (_sub_agent_publisher, sub_agent_consumer) = pub_sub();
        let _http_server_runner = Runner::new(
            ServerConfig {
                enabled: true,
                port: 0.into(),
                ..Default::default()
            },
            runtime,
            agent_control_consumer,
            sub_agent_consumer,
            None,
        );
        // server warm up
        sleep(Duration::from_millis(100));

        drop(_agent_control_publisher);
        drop(_sub_agent_publisher);

        // wait for logs to be flushed
        sleep(Duration::from_millis(100));
        assert!(logs_with_scope_contain(
            "newrelic_agent_control::agent_control::http_server::server",
            "status server gracefully stopped",
        ));
    }
}
