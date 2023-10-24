use kube::Client;

use crate::common::{K8sCluster, K8sEnv};
use std::{thread, time::Duration};

// tokio test runs with 1 thread by default causing deadlock when executing `block_on` code (clean-up).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs k8s cluster"]
async fn k8s_test_using_local_minikube() -> Result<(), Box<dyn std::error::Error>> {
    let mut test = K8sEnv::new().await?;

    example(test.test_namespace().await?.as_str()).await;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "spawns a k8s cluster"]
async fn k8s_spawning_container_k3s() -> Result<(), Box<dyn std::error::Error>> {
    let _test = K8sCluster::new().await?;

    example("default").await;

    Ok(())
}

// Just a test example that should be removed.
async fn example(namespace: &str) {
    use k8s_openapi::api::core::v1::Pod;
    use kube::api::{Api, ApiResource, NotUsed, Object, PostParams, ResourceExt};
    use serde::Deserialize;

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

    thread::sleep(Duration::from_secs(2));
    // Here we replace heavy type k8s_openapi::api::core::v1::PodSpec with
    #[derive(Clone, Deserialize, Debug)]
    struct PodSpecSimple {
        containers: Vec<ContainerSimple>,
    }
    #[derive(Clone, Deserialize, Debug)]
    struct ContainerSimple {
        #[allow(dead_code)]
        image: String,
    }
    type PodSimple = Object<PodSpecSimple, NotUsed>;

    // Here we simply steal the type info from k8s_openapi, but we could create this from scratch.
    let ar = ApiResource::erase::<k8s_openapi::api::core::v1::Pod>(&());

    let pods: Api<PodSimple> = Api::namespaced_with(client, namespace, &ar);
    for p in pods.list(&Default::default()).await.unwrap() {
        print!("Pod {} runs: {:?}", p.name_any(), p.spec.containers);
        assert_eq!(p.name_any(), String::from("example"))
    }
}
