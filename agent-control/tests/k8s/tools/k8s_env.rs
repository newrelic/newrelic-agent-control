use crate::common::{global_logger::init_logger, runtime::tokio_runtime};

use super::test_crd::create_foo_crd;
use futures::TryStreamExt;
use k8s_openapi::api::core::v1::{Namespace, Pod};
use k8s_openapi::api::rbac::v1::ClusterRole;
use kube::runtime::conditions::is_pod_running;
use kube::runtime::wait::Error as KubeWaitError;
use kube::runtime::wait::await_condition;
use kube::{
    Api, Client, ResourceExt,
    api::{DeleteParams, PostParams},
};
use newrelic_agent_control::http::tls::install_rustls_default_crypto_provider;
use std::net::SocketAddr;
use std::{env, sync::Once};
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::TcpListenerStream;
use tracing::{error, info};

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

pub const KUBECONFIG_PATH: &str = "tests/k8s/.kubeconfig-dev";

pub static INIT_RUSTLS: Once = Once::new();

/// This struct represents a running k8s cluster and it provides utilities to handle multiple namespaces, and
/// resources are cleaned-up when the object is dropped.
/// The `Foo` CR is created automatically, therefore any test using this component can assume it exits.
pub struct K8sEnv {
    pub client: Client,
    generated_namespaces: Vec<String>,
    port_forwarder_handle: Option<JoinHandle<()>>,
}

impl K8sEnv {
    pub async fn new() -> Self {
        INIT_RUSTLS.call_once(|| {
            install_rustls_default_crypto_provider();
        });
        init_logger();

        // Forces the client to use the dev kubeconfig file.
        unsafe { env::set_var("KUBECONFIG", KUBECONFIG_PATH) };

        let client = Client::try_default().await.expect("fail to create client");
        create_foo_crd(client.to_owned()).await;

        K8sEnv {
            client,
            generated_namespaces: Vec::new(),
            port_forwarder_handle: None,
        }
    }

    /// Creates and returns a namespace for testing purposes, it will be deleted when the [K8sEnv] object is dropped.
    pub async fn test_namespace(&mut self) -> String {
        let mut test_namespace = Namespace::default();
        test_namespace.metadata.generate_name = Some("ac-test-".to_string());

        let namespaces: Api<Namespace> = Api::all(self.client.clone());

        let created_namespace = namespaces
            .create(&PostParams::default(), &test_namespace)
            .await
            .expect("fail to create test namespace");

        let ns = created_namespace
            .metadata
            .name
            .ok_or("fail getting the ns")
            .unwrap();

        self.generated_namespaces.push(ns.clone());

        ns
    }

    pub fn port_forward(&mut self, pod_name: &str, local_port: u16, pod_port: u16) {
        let pod_name = pod_name.to_string();
        let client = self.client.clone();

        let handle = tokio_runtime().spawn(async move {
            if let Err(e) = async {
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

        self.port_forwarder_handle = Some(handle);
    }
}

impl Drop for K8sEnv {
    fn drop(&mut self) {
        // clean up test environment even if the test panics.
        // 'async drop' doesn't exist so `block_on` is needed to run it synchronously.
        //
        // Since K8sEnv variables can be dropped from either sync or async code, we need an additional runtime to make
        // it work.
        //
        // `futures::executor::block_on` is needed because we cannot execute `runtime.block_on` from a tokio
        // context (such as `#[tokio::test]`) as it would fail with:
        // ```
        // 'Cannot start a runtime from within a runtime. This happens because a function (like `block_on`) attempted to block the current thread while the thread is being used to drive asynchronous tasks.'
        // ````
        // It is important to notice that the usage of `futures::executor::block_on` could lead to a dead-lock if there
        // are not available threads in the tokio runtime, so we need to use the multi-threading version of the macro:
        // `#[tokio::test(flavor = "multi_thread")]`
        //
        // `runtime.spawn(<future-block>).await` is needed because we cannot execute `futures::executor::block_on` when there is
        // no tokio runtime (synchronous tests), since it would fail with:
        // ```
        // 'there is no reactor running, must be called from the context of a Tokio 1.x runtime
        // ```
        if let Some(handle) = self.port_forwarder_handle.take() {
            handle.abort();
        }

        futures::executor::block_on(async move {
            let ns_api: Api<Namespace> = Api::all(self.client.clone());
            let cr_api: Api<ClusterRole> = Api::all(self.client.clone());

            let generated_namespaces = self.generated_namespaces.clone();
            tokio_runtime()
                .spawn(async move {
                    for ns in generated_namespaces.into_iter() {
                        ns_api
                            .delete(ns.as_str(), &DeleteParams::default())
                            .await
                            .expect("fail to remove namespace");
                    }

                    // TODO This is a workaround. As soon as we have a way to configure RELEASE_NAME in the tests, we can remove this.
                    let _ = cr_api
                        .delete(
                            "agent-control-deployment-resources",
                            &DeleteParams::default(),
                        )
                        .await;
                })
                .await
                .unwrap();
        })
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
