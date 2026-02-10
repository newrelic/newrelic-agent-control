use crate::agent_control::config::OpAMPClientConfig;
use crate::agent_control::http_server::StatusServerError;
use crate::agent_control::http_server::async_bridge::run_async_sync_bridge;
use crate::agent_control::http_server::config::ServerConfig;
use crate::event::cancellation::CancellationMessage;
use crate::event::channel::EventConsumer;
use crate::event::{AgentControlEvent, SubAgentEvent};
use crate::utils::thread_context::{NotStartedThreadContext, StartedThreadContext};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use tracing::dispatcher;
use tracing::{debug, error, info};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

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
    pub fn start(self) -> Result<StartedHttpServer, StatusServerError> {
        // Create outer channel for timeout support in sync context
        let (startup_publisher, startup_consumer) = std::sync::mpsc::channel();

        let dispatch = dispatcher::get_default(|d| d.clone());
        let span = tracing::Span::current();

        let callback = move |stop_consumer: EventConsumer<CancellationMessage>| {
            let _guard = dispatcher::set_default(&dispatch);
            let _enter = span.enter();

            self.spawn_server(stop_consumer, startup_publisher)
        };

        let thread_context = NotStartedThreadContext::new("Http server", callback).start();

        info!("Waiting for the HTTP status server to start");
        let startup_result =
            startup_consumer
                .recv_timeout(STARTUP_TIMEOUT)
                .map_err(|err| match err {
                    std::sync::mpsc::RecvTimeoutError::Timeout => {
                        StatusServerError::StartupTimeout(STARTUP_TIMEOUT)
                    }
                    std::sync::mpsc::RecvTimeoutError::Disconnected => {
                        StatusServerError::StartupChannelClosed
                    }
                })?;

        startup_result.map_err(StatusServerError::BindError)?;
        info!("HTTP status server started");

        Ok(StartedHttpServer {
            thread_context: Some(thread_context),
        })
    }

    fn spawn_server(
        self,
        stop_rx: EventConsumer<CancellationMessage>,
        startup_publisher: std::sync::mpsc::Sender<Result<(), String>>,
    ) {
        // Create 2 unbounded channel to send the Agent Control and Sub Agent Sync events
        // to the Async Status Server
        let (async_agent_control_event_publisher, async_agent_control_event_consumer) =
            tokio::sync::mpsc::unbounded_channel::<AgentControlEvent>();
        let (async_sub_agent_event_publisher, async_sub_agent_event_consumer) =
            tokio::sync::mpsc::unbounded_channel::<SubAgentEvent>();

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
                    startup_publisher,
                ),
            )
            .inspect_err(|err| {
                error!(error_msg = %err, "error running status server");
            });

        // Wait until the bridge is closed
        bridge_join_handle.join().unwrap();
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
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    use assert_matches::assert_matches;

    use crate::agent_control::http_server::config::ServerConfig;
    use crate::agent_control::run::runtime::tests::tokio_runtime;
    use crate::event::AgentControlEvent;
    use crate::event::channel::pub_sub;
    use crate::utils::retry::retry;

    use super::*;

    #[test]
    fn test_server_stops_gracefully_when_dropped() {
        let runtime = tokio_runtime();
        let port = get_available_port();
        let (_agent_control_publisher, agent_control_consumer) = pub_sub::<AgentControlEvent>();
        let (_sub_agent_publisher, sub_agent_consumer) = pub_sub();

        let started_http_server = Runner::new(
            ServerConfig {
                enabled: true,
                port: port.into(),
                ..Default::default()
            },
            runtime.clone(),
            agent_control_consumer,
            sub_agent_consumer,
            None,
        )
        .start()
        .expect("HTTP server should start successfully");

        assert_status_endpoint(port);

        drop(started_http_server);

        assert_port_is_released(port);
    }

    #[test]
    fn test_server_stops_gracefully_when_external_channels_close() {
        let runtime = tokio_runtime();
        let port = get_available_port();
        let (_agent_control_publisher, agent_control_consumer) = pub_sub::<AgentControlEvent>();
        let (_sub_agent_publisher, sub_agent_consumer) = pub_sub();

        let _http_server_runner = Runner::new(
            ServerConfig {
                enabled: true,
                port: port.into(),
                ..Default::default()
            },
            runtime,
            agent_control_consumer,
            sub_agent_consumer,
            None,
        )
        .start()
        .expect("HTTP server should start successfully");

        assert_status_endpoint(port);

        // Drop the publishers to trigger shutdown
        drop(_agent_control_publisher);
        drop(_sub_agent_publisher);

        assert_port_is_released(port);
    }

    #[test]
    fn test_server_returns_error_on_bind_failure() {
        let runtime = tokio_runtime();

        // Bind a port to simulate it being in use
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // Try to start the HTTP server on the already-bound port
        let (_agent_control_publisher, agent_control_consumer) = pub_sub::<AgentControlEvent>();
        let (_sub_agent_publisher, sub_agent_consumer) = pub_sub();
        let result = Runner::new(
            ServerConfig {
                enabled: true,
                port: port.into(),
                ..Default::default()
            },
            runtime,
            agent_control_consumer,
            sub_agent_consumer,
            None,
        )
        .start();

        // The server should fail to start
        assert_matches!(result.err().unwrap(), StatusServerError::BindError(_));
    }

    /// Helper to find an available port by binding and immediately releasing it
    fn get_available_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener); // just releasing it explicitly
        port
    }

    /// Helper to check if the status endpoint responds successfully
    fn assert_status_endpoint(port: u16) {
        let response = reqwest::blocking::get(format!("http://127.0.0.1:{}/status", port))
            .expect("The server should be up and running");
        assert!(response.status().is_success());
    }

    // Helper to check that the server is down
    fn assert_port_is_released(port: u16) {
        retry(20, Duration::from_millis(100), || {
            if TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
                Err(format!("the server is still up on {port}"))
            } else {
                Ok(())
            }
        })
        .expect("The port should be released")
    }
}
