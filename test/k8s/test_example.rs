use k8s_openapi::api::core::v1::Pod;
use kube::Client;

use crate::common::{K8sCluster, K8sEnv};

use kube::api::Api;

// tokio test runs with 1 thread by default causing deadlock when executing `block_on` code during test helper drop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs k8s cluster"]
async fn k8s_test_using_local_minikube() {
    let mut test = K8sEnv::new().await;

    let test_ns = test.test_namespace().await;

    fake_binary_run_example(test_ns.as_str()).await;

    let pods: Api<Pod> = Api::namespaced(test.client.to_owned(), test_ns.as_str());
    pods.get("example").await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "spawns a k8s cluster"]
async fn k3s_spawning_container_k3s() {
    let test = K8sCluster::new().await;

    fake_binary_run_example("default").await;

    let pods: Api<Pod> = Api::namespaced(test.client.to_owned().unwrap(), "default");
    pods.get("example").await.unwrap();
}

// Just a test example that should be removed.
async fn fake_binary_run_example(namespace: &str) {
    use kube::api::PostParams;

    let client = Client::try_default().await.unwrap();

    let p: Pod = serde_yaml::from_str(
        r#"apiVersion: v1
kind: Pod
metadata:
  name: example
spec:
  containers:
  - name: example
    image: alpine
    command:
    - tail
    - "-f"
    - "/dev/null"
"#,
    )
    .unwrap();

    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    // Stop on error including a pod already exists or still being deleted.
    pods.create(&PostParams::default(), &p).await.unwrap();
}
