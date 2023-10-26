use kube::client::ConfigExt;
use kube::config::KubeConfigOptions;
use kube::{Client, Config, Error};
use tower::ServiceBuilder;
use tracing::{debug, info};

#[derive(thiserror::Error, Debug)]
pub enum K8sClientConfigError {
    #[error("it is not possible to create a k8s client: `{0}`")]
    UnableToSetupClient(String),
}

impl From<kube::Error> for K8sClientConfigError {
    fn from(value: Error) -> Self {
        K8sClientConfigError::UnableToSetupClient(value.to_string())
    }
}

/// Constructs a new Kubernetes client.
///
/// If loading from the inCluster config fail we fall back to kube-config
/// This will respect the `$KUBECONFIG` envvar, but otherwise default to `~/.kube/config`.
///
pub async fn create_client() -> Result<Client, K8sClientConfigError> {
    debug!("trying inClusterConfig for k8s client");
    let in_cluster_config = Config::incluster();
    if let Ok(config) = in_cluster_config {
        let c = create_client_from_config(config)?;
        debug!("inClusterConfig client creation succeeded");
        return Ok(c);
    } else {
        debug!("inClusterConfig failed since we are not running in a cluster")
    }

    debug!("trying kubeconfig for k8s client");
    let c = KubeConfigOptions {
        ..Default::default()
    };
    let kubeconfig = Config::from_kubeconfig(&c).await;
    if let Ok(config) = kubeconfig {
        let c = create_client_from_config(config)?;
        debug!("kubeconfig client creation succeeded");
        return Ok(c);
    }
    Err(K8sClientConfigError::UnableToSetupClient(
        "unable to create a k8s client".to_string(),
    ))
}

fn create_client_from_config(config: Config) -> Result<Client, K8sClientConfigError> {
    let https = config.openssl_https_connector();
    if https.is_err() {
        return Err(K8sClientConfigError::UnableToSetupClient(
            "creating openssl_https_connector".to_string(),
        ));
    }
    let service = ServiceBuilder::new()
        .layer(config.base_uri_layer())
        .service(hyper::Client::builder().build(https?));
    info!(
        "Configured cluster url={}, namespace={}",
        config.cluster_url, config.default_namespace
    );
    Ok(Client::new(service, config.default_namespace))
}
