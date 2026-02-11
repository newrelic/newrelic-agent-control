use crate::agent_control::config::OpAMPClientConfig;
use crate::agent_control::http_server::StatusServerError;
use crate::agent_control::http_server::config::{DEFAULT_WORKERS, ServerConfig};
use crate::agent_control::http_server::status::Status;
use crate::agent_control::http_server::status_handler::status_handler;
use crate::agent_control::http_server::status_updater::on_agent_control_event_update_status;
use crate::event::{AgentControlEvent, SubAgentEvent};
use actix_web::{App, HttpServer, dev::ServerHandle, web};
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::task::JoinError;
use tracing::{debug, error, info};

/// Helper struct to manage [JoinError] when shutting down the server
#[derive(Default)]
struct JoinHandleErrors {
    server: Option<JoinError>,
    events: Option<JoinError>,
}

impl JoinHandleErrors {
    fn is_empty(&self) -> bool {
        self.server.is_none() && self.events.is_none()
    }
}

impl std::fmt::Display for JoinHandleErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let messages: Vec<String> = [&self.server, &self.events]
            .into_iter()
            .filter_map(|err| err.as_ref().map(|err| err.to_string()))
            .collect();
        write!(f, "{}", messages.join(","))
    }
}

pub async fn run_status_server(
    server_config: ServerConfig,
    agent_control_event_consumer: tokio::sync::mpsc::UnboundedReceiver<AgentControlEvent>,
    sub_agent_event_consumer: tokio::sync::mpsc::UnboundedReceiver<SubAgentEvent>,
    maybe_opamp_client_config: Option<OpAMPClientConfig>,
    startup_publisher: std::sync::mpsc::Sender<Result<(), String>>,
) -> Result<(), StatusServerError> {
    // channel to share the Server handle between "threads". This way we can
    // get the Server in the main "thread" and stop the Server once the
    // event process loop finishes.
    let (server_handle_publisher, server_handle_consumer) = std::sync::mpsc::channel();

    // structure to contain the status of the Agent Control. It will be written
    // by the Event Processor on Agent Control Events, and read from the
    // HTTP Server
    let status = if let Some(opamp_config) = maybe_opamp_client_config {
        Status::default().with_opamp(opamp_config.endpoint)
    } else {
        Status::default()
    };

    let status = Arc::new(tokio::sync::RwLock::new(status));

    // Tokio Runtime
    let rt = Handle::current();

    debug!("spawning thread for the event processor");
    let status_clone = status.clone();
    let event_join_handle = rt.spawn(on_agent_control_event_update_status(
        agent_control_event_consumer,
        sub_agent_event_consumer,
        status_clone,
    ));

    debug!("spawning thread for status server");
    let status_clone = status.clone();
    let server_join_handle = rt.spawn(async {
        let _ = run_server(
            server_config,
            server_handle_publisher,
            status_clone,
            startup_publisher,
        )
        .await
        .inspect_err(|err| {
            error!(error_msg = %err, "starting HTTP server");
        });
    });

    let mut join_handle_errors = JoinHandleErrors::default();
    debug!("waiting for the event_join_handle");
    if let Err(err) = event_join_handle.await {
        join_handle_errors.events = Some(err);
    };
    debug!("event_join_handle finished");

    // The server could have failed to start and in that case the channel will be closed.
    if let Ok(server_handle) = server_handle_consumer.recv() {
        debug!("stopping status server");
        server_handle.stop(true).await;
        debug!("status server stopped succeeded");
    }

    debug!("waiting for status server join handle");
    if let Err(err) = server_join_handle.await {
        join_handle_errors.server = Some(err);
    };

    debug!("status server gracefully stopped");

    if join_handle_errors.is_empty() {
        Ok(())
    } else {
        Err(StatusServerError::JoinHandleError(
            join_handle_errors.to_string(),
        ))
    }
}

async fn run_server(
    server_config: ServerConfig,
    tx: std::sync::mpsc::Sender<ServerHandle>,
    status: Arc<tokio::sync::RwLock<Status>>,
    startup_publisher: std::sync::mpsc::Sender<Result<(), String>>,
) -> std::io::Result<()> {
    info!(
        "starting HTTP server at http://{}:{}",
        server_config.host, server_config.port
    );

    let status_data = web::Data::new(status);

    let server = match HttpServer::new(move || {
        App::new()
            .app_data(status_data.clone())
            // TODO Do we want to log the requests?
            // The line below logs all requests as info level
            // .wrap(middleware::Logger::default())
            .service(web::resource("/status").to(status_handler))
    })
    .bind((server_config.host.to_string(), server_config.port.into()))
    {
        Ok(server) => server,
        Err(err) => {
            // Signal startup failure (clone the error info for the channel)
            let _ = startup_publisher.send(Err(err.to_string()));
            return Err(err);
        }
    };

    let server = server.workers(DEFAULT_WORKERS).run();

    // Send server handle back to the main thread
    let _ = tx.send(server.handle());

    // Signal successful startup
    let _ = startup_publisher.send(Ok(()));

    server.await
}
