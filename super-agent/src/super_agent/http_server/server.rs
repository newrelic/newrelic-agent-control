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
    opamp_client_config: OpAMPClientConfig,
) -> Result<(), StatusServerError> {
    // channel to share the Server handle between "threads". This way we can
    // get the Server in the main "thread" and stop the Server once the
    // event process loop finishes.
    let (server_handle_publisher, server_handle_consumer) = mpsc::channel();

    // structure to contain the status of the Super Agent. It will be written
    // by the Event Processor on Super Agent Events, and read from the
    // HTTP Server
    let status = if opamp_client_config.is_enabled() {
        Status::default().with_opamp(opamp_client_config.endpoint)
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
    let server_join_handle = rt.spawn(async {
        let _ = run_server(server_config, server_handle_publisher, status_clone)
            .await
            .inspect_err(|err| {
                error!(error_msg = %err, "starting HTTP server");
            });
    });

    debug!("waiting for the event_join_handle");
    event_join_handle.await?;
    debug!("event_join_handle succeeded");

    // The server could have failed to start and in that case the channel will be closed.
    if let Ok(server_handle) = server_handle_consumer.recv() {
        debug!("stopping status server");
        server_handle.stop(true).await;
        debug!("status server stopped succeeded");
    }

    debug!("waiting for status server join handle");
    server_join_handle.await?;

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
