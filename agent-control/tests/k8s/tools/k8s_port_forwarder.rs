use crate::common::runtime::tokio_runtime;
use futures::TryStreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::runtime::wait::Error as KubeWaitError;
use kube::{
    Client, ResourceExt,
    api::Api,
    runtime::wait::{await_condition, conditions::is_pod_running},
};
use newrelic_agent_control::http::tls::install_rustls_default_crypto_provider;
use std::env;
use std::net::SocketAddr;
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::TcpListenerStream;
use tracing::*;

#[derive(Debug, Error)]
enum PortForwardError {
    #[error("kube wait error `{0}`")]
    KubeWaitError(#[from] KubeWaitError),

    #[error("pod not found: {0}")]
    PodNotFound(String),

    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    #[error("operation timed out: {0}")]
    Timeout(String),

    #[error("an error occurred: {0}")]
    Other(String),
}

pub struct PortForwardServer {
    handle: JoinHandle<()>,
}

impl PortForwardServer {
    /// Starts a new PortForwardServer launching a server in localhost with the specified local_port
    /// that forwards the stream from the pod_port port from a pod with name pod_name
    pub fn start_new(pod_name: &str, local_port: u16, pod_port: u16) -> Self {
        let pod_name = pod_name.to_string();
        let handle = tokio_runtime().spawn(async move {
            if let Err(e) = async {
                crate::k8s::tools::k8s_env::INIT_RUSTLS.call_once(|| {
                    install_rustls_default_crypto_provider();
                });

                // Forces the client to use the dev kubeconfig file.
                unsafe { env::set_var("KUBECONFIG", crate::k8s::tools::k8s_env::KUBECONFIG_PATH) };

                let client = Client::try_default().await.map_err(|e| {
                    PortForwardError::Other(format!("fail to create client: {}", e))
                })?;
                let pods: Api<Pod> = Api::default_namespaced(client);

                let p = pods
                    .get(&pod_name)
                    .await
                    .map_err(|_| PortForwardError::PodNotFound(pod_name.clone()))?;
                info!("Found pod: {}", p.name_any());

                let running = await_condition(pods.clone(), &pod_name, is_pod_running());
                tokio::time::timeout(std::time::Duration::from_secs(60), running)
                    .await
                    .map_err(|_| {
                        PortForwardError::Timeout("Pod running check timed out".to_string())
                    })??;
                info!("Pod is running");

                let addr = SocketAddr::from(([127, 0, 0, 1], local_port));
                let server =
                    TcpListenerStream::new(TcpListener::bind(addr).await.map_err(|e| {
                        PortForwardError::ConnectionFailed(format!("Failed to bind: {}", e))
                    })?)
                    .try_for_each(|client_conn| async {
                        if let Ok(peer_addr) = client_conn.peer_addr() {
                            info!(%peer_addr, "new connection");
                        }
                        let pods = pods.clone();
                        let pod_name = pod_name.clone();
                        // Spawn a new task to forward the connection to the pod.
                        tokio::spawn(async move {
                            if let Err(e) =
                                forward_connection(&pods, &pod_name, pod_port, client_conn).await
                            {
                                error!(
                                    error = e.to_string().as_str(),
                                    "failed to forward connection"
                                );
                            }
                        });
                        Ok(())
                    });

                server
                    .await
                    .map_err(|e| PortForwardError::Other(format!("Server error: {}", e)))?;
                info!("Shutting down");
                Ok::<(), PortForwardError>(())
            }
            .await
            {
                error!(error = %e, "server error");
            }
        });

        PortForwardServer { handle }
    }

    fn stop(&self) {
        self.handle.abort();
    }
}

impl Drop for PortForwardServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// forward_connection forwards the stream from calling porforward on a pod in a specific port
/// to the TcpStream passed in client_conn
async fn forward_connection(
    pods: &Api<Pod>,
    pod_name: &str,
    port: u16,
    mut client_conn: TcpStream,
) -> Result<(), PortForwardError> {
    let mut forwarder = pods
        .portforward(pod_name, &[port])
        .await
        .map_err(|_| PortForwardError::PodNotFound(pod_name.to_string()))?;

    let mut upstream_conn = forwarder
        .take_stream(port)
        .ok_or_else(|| PortForwardError::Other("port not found in forwarder".to_string()))?;

    tokio::io::copy_bidirectional(&mut client_conn, &mut upstream_conn)
        .await
        .map_err(|_| PortForwardError::ConnectionFailed("failed to copy data".to_string()))?;

    drop(upstream_conn);
    forwarder
        .join()
        .await
        .map_err(|_| PortForwardError::Timeout("failed to join forwarder".to_string()))?;

    info!("connection closed");
    Ok(())
}
