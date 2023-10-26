use kube::config::{KubeConfigOptions, KubeconfigError};
use kube::{Client, Config, Error};
use tracing::debug;

#[derive(thiserror::Error, Debug)]
pub enum K8sClientConfigError {
    #[error("it is not possible to create a k8s client")]
    UnableToSetupClient,

    #[error("it is not possible to create a k8s client due to ssl: `{0}`")]
    UnableToSetupClientSSL(#[from] Error),

    #[error("it is not possible to read kubeconfig: `{0}`")]
    UnableToSetupClientKubeconfig(#[from] KubeconfigError),
}
/// Constructs a new Kubernetes client.
///
/// If loading from the inCluster config fail we fall back to kube-config
/// This will respect the `$KUBECONFIG` envvar, but otherwise default to `~/.kube/config`.
/// Not leveraging infer() to check inClusterConfig first
///
pub async fn create_client() -> Result<Client, K8sClientConfigError> {
    debug!("trying inClusterConfig for k8s client");
    let config = Config::incluster().unwrap_or({
        debug!("inClusterConfig failed, trying kubeconfig for k8s client");
        let c = KubeConfigOptions {
            ..Default::default()
        };

        Config::from_kubeconfig(&c).await?
    });

    let c = Client::try_from(config)?;
    debug!("client creation succeeded");
    Ok(c)
}
