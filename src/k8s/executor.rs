use k8s_openapi::api::core::v1::Pod;
use kube::config::{KubeConfigOptions, KubeconfigError};
use kube::{api::ListParams, Api, Client, Config, Error};
use mockall::*;
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

#[derive(thiserror::Error, Debug)]
pub enum K8sClientRequestError {
    #[error("it is not possible to fetch data")]
    UnableToFetchData(#[from] Error),
}

#[derive(Clone)]
pub struct K8sExecutor {
    client: Client,
}

#[automock]
impl K8sExecutor {
    /// Constructs a new Kubernetes client.
    ///
    /// If loading from the inCluster config fail we fall back to kube-config
    /// This will respect the `$KUBECONFIG` envvar, but otherwise default to `~/.kube/config`.
    /// Not leveraging infer() to check inClusterConfig first
    ///
    pub async fn try_default() -> Result<K8sExecutor, K8sClientConfigError> {
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
        Ok(K8sExecutor::new(c))
    }

    pub fn new(c: Client) -> K8sExecutor {
        K8sExecutor { client: c }
    }

    pub async fn get_minor_version(&self) -> Result<String, K8sClientRequestError> {
        let version = self.client.apiserver_version().await?;
        Ok(version.minor)
    }

    pub async fn get_pods(&self) -> Result<Vec<Pod>, K8sClientRequestError> {
        let pod_client: Api<Pod> = Api::default_namespaced(self.client.clone());
        let pod_list = pod_client.list(&ListParams::default()).await?;
        Ok(pod_list.items)
    }
}
