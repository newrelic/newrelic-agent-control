use crate::common::{block_on, start_super_agent, K8sEnv};

use std::error::Error;
use std::path::Path;
use std::time::Duration;

use k8s_openapi::api::apps::v1::Deployment;
use kube::{api::Api, Client};
use tokio::time::sleep;

#[test]
#[ignore = "needs a k8s cluster"]
fn k8s_sub_agent_started() {
    let file_path = Path::new("test/k8s/data/static.yml");
    let mut child = start_super_agent(file_path, Some("test/k8s/data"));

    // Setup k8s env
    let k8s = block_on(K8sEnv::new());

    let deployment_name = "open-telemetry-opentelemetry-collector";
    let namespace = "default";
    let max_retries = 30;
    let duration = Duration::from_millis(5000);

    // Check deployment is created with retry.
    let result = block_on(check_deployment_exists(
        k8s.client.clone(),
        deployment_name,
        namespace,
        max_retries,
        duration,
    ));

    assert!(
        result.is_ok(),
        "Deployment does not exist or could not be verified"
    );

    child.kill().expect("Failed to kill child process");
}

async fn check_deployment_exists(
    client: Client,
    name: &str,
    namespace: &str,
    max_retries: usize,
    retry_interval: Duration,
) -> Result<(), Box<dyn Error>> {
    let api: Api<Deployment> = Api::namespaced(client, namespace);
    for _ in 0..max_retries {
        match api.get(name).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                println!("Error checking deployment {}: {:?}, retrying...", name, e);
                sleep(retry_interval).await;
            }
        }
    }
    Err(Box::new(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "CR not found after retries",
    )))
}
