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
        Ok(c)
    } else {
        Err(K8sClientConfigError::UnableToSetupClient(
            "Unable to create a k8s client".to_string(),
        ))
    }
}

fn create_client_from_config(config: Config) -> Result<Client, K8sClientConfigError> {
    let https = config.openssl_https_connector()?;
    let service = ServiceBuilder::new()
        .layer(config.base_uri_layer())
        .service(hyper::Client::builder().build(https));
    info!(
        "Configured cluster url={}, namespace={}",
        config.cluster_url, config.default_namespace
    );
    Ok(Client::new(service, config.default_namespace))
}

#[cfg(test)]
mod tests {
    use crate::k8s::client::create_client;
    use http::Method;
    use k8s_openapi::serde_json;
    use kube::Client;
    use tower_test::mock;

    // #[tokio::test]
    // async fn integration_test_using_client() {
    //     let c = create_client().await;
    //
    //     let version = c.unwrap().apiserver_version().await;
    //
    //     assert_eq!(true, version.is_ok());
    //     let v = version.unwrap();
    //     assert_eq!(v.platform, "linux/amd64");
    //     assert_eq!(v.minor, "24");
    // }

    #[tokio::test]
    async fn simple_test_with_http_mock() {
        let (mock_service, handle) =
            mock::pair::<http::Request<hyper::Body>, http::Response<hyper::Body>>();
        let mock_client = Client::new(mock_service, "default");
        ApiServerVerifier(handle).run(Scenario::Version);

        let version = mock_client.apiserver_version().await;

        assert_eq!(true, version.is_ok());
        let v = version.unwrap();
        assert_eq!(v.platform, "linux/amd64");
        assert_eq!(v.minor, "24");
    }

    type ApiServerHandle = mock::Handle<http::Request<hyper::Body>, http::Response<hyper::Body>>;

    struct ApiServerVerifier(ApiServerHandle);

    /// Scenarios we test for in ApiServerVerifier above
    enum Scenario {
        Version,
    }

    impl ApiServerVerifier {
        fn run(mut self, scenario: Scenario) -> tokio::task::JoinHandle<()> {
            tokio::spawn(async move {
                match scenario {
                    Scenario::Version => {
                        let (request, send) =
                            self.0.next_request().await.expect("service not called 1");
                        assert_eq!(request.method(), Method::GET);

                        let data = serde_json::json!({
                          "major": "1",
                          "minor": "24",
                          "gitVersion": "v1.24.15-gke.1700",
                          "gitCommit": "8cadcdb5605ddc1b77a0b1dd3fbd8182a23f58ae",
                          "gitTreeState": "clean",
                          "buildDate": "2023-07-17T09:27:42Z",
                          "goVersion": "go1.19.10 X:boringcrypto",
                          "compiler": "gc",
                          "platform": "linux/amd64"
                        });
                        let response = serde_json::to_vec(&data).unwrap();

                        send.send_response(
                            http::Response::builder()
                                .body(hyper::Body::from(response))
                                .unwrap(),
                        );
                    }
                }
            })
        }
    }
}
