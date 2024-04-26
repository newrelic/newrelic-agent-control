use crate::event::SuperAgentEvent;
use crate::super_agent::config::OpAMPClientConfig;
use crate::super_agent::http_server::config::{ServerConfig, DEFAULT_WORKERS};
use crate::super_agent::http_server::status::Status;
use crate::super_agent::http_server::status_handler::status_handler;
use crate::super_agent::http_server::status_updater::on_super_agent_event_update_status;
use crate::super_agent::http_server::StatusServerError;
use actix_web::{dev::ServerHandle, web, App, HttpServer};
use std::sync::mpsc;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

pub async fn run_status_server(
    server_config: ServerConfig,
    sa_event_consumer: UnboundedReceiver<SuperAgentEvent>,
    maybe_opamp_client_config: Option<OpAMPClientConfig>,
) -> Result<(), StatusServerError> {
    // channel to share the Server handle between "threads". This way we can
    // get the Server in the main "thread" and stop the Server once the
    // event process loop finishes.
    let (server_handle_publisher, server_handle_consumer) = mpsc::channel();

    // structure to contain the status of the Super Agent. It will be written
    // by the Event Processor on Super Agent Events, and read from the
    // HTTP Server
    let status = if let Some(opamp_config) = maybe_opamp_client_config {
        Status::default().with_opamp(opamp_config.endpoint)
    } else {
        Status::default()
    };

    let status = Arc::new(RwLock::new(status));

    // Tokio Runtime
    let rt = Handle::current();

    debug!("spawning thread for the event processor");
    let status_clone = status.clone();
    let event_join_handle = rt.spawn(on_super_agent_event_update_status(
        sa_event_consumer,
        status_clone,
    ));

    debug!("spawning thread for status server");
    let status_clone = status.clone();
    let server_join_handle = rt.spawn(run_server(
        server_config,
        server_handle_publisher,
        status_clone,
    ));

    // Get the Server Handle so we can stop it later
    let server_handle = server_handle_consumer.recv()?;

    debug!("waiting for the event_join_handle");
    event_join_handle.await?;
    debug!("event_join_handle succeeded");

    debug!("stopping status server");
    server_handle.stop(true).await;
    debug!("status server stopped succeeded");

    debug!("waiting for status server join handle");
    _ = server_join_handle
        .await?
        .inspect_err(|e| error!(error_msg = e.to_string(), "error in server_join_handle"));

    Ok(())
}

async fn run_server(
    server_config: ServerConfig,
    tx: mpsc::Sender<ServerHandle>,
    status: Arc<RwLock<Status>>,
) -> std::io::Result<()> {
    info!(
        "starting HTTP server at http://{}:{}",
        server_config.host, server_config.port
    );

    let status_data = web::Data::new(status);

    let server = HttpServer::new(move || {
        App::new()
            .app_data(status_data.clone())
            // TODO Do we want to log the requests?
            // The line below logs all requests as info level
            // .wrap(middleware::Logger::default())
            .service(web::resource("/status").to(status_handler))
    })
    .bind((server_config.host.to_string(), server_config.port.into()))?
    .workers(DEFAULT_WORKERS)
    .run();

    // Send server handle back to the main thread
    let _ = tx.send(server.handle());

    server.await
}
