use kube::client::ConfigExt;
use kube::config::KubeConfigOptions;
use kube::{Client, Config, Error};
use tower::ServiceBuilder;
use tracing::{debug, info};

#[derive(thiserror::Error, Debug)]
pub enum K8sClientConfigError {
    #[error("it is not possible to create a k8s client")]
    UnableToSetupClient,

    #[error("it is not possible to create a k8s client due to ssl: `{0}`")]
    UnableToSetupClientSSL(#[from] Error),
}
/// Constructs a new Kubernetes client.
///
/// If loading from the inCluster config fail we fall back to kube-config
/// This will respect the `$KUBECONFIG` envvar, but otherwise default to `~/.kube/config`.
/// Not leveraging infer() to check inClusterConfig first
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
    let config = Config::from_kubeconfig(&c).await.map_err(|| {
        return Err(K8sClientConfigError::UnableToSetupClient);
    })?;
    let c = create_client_from_config(config)?;
    debug!("kubeconfig client creation succeeded");
    return Ok(c);
}

///
/// Using a custom function and not https://docs.rs/kube/0.86.0/kube/struct.Client.html#method.try_from
/// to add the openssl implementation.
///
fn create_client_from_config(config: Config) -> Result<Client, K8sClientConfigError> {
    let https = config.openssl_https_connector().map_err(|err| {
        return Err(K8sClientConfigError::UnableToSetupClientSSL(err));
    })?;
    let service = ServiceBuilder::new()
        .layer(config.base_uri_layer())
        .service(hyper::Client::builder().build(https));
    info!(
        "Configured cluster url={}, namespace={}",
        config.cluster_url, config.default_namespace
    );
    Ok(Client::new(service, config.default_namespace))
}
