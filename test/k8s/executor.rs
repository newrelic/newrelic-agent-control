use crate::common::{create_foo_crd, Foo, FooSpec, K8sCluster, K8sEnv};
use k8s_openapi::api::core::v1::Pod;
use kube::{api::Api, core::GroupVersionKind};

use newrelic_super_agent::k8s::executor::K8sExecutor;

// tokio test runs with 1 thread by default causing deadlock when executing `block_on` code during test helper drop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs k8s cluster"]
async fn k8s_create_dynamic_resource() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    create_foo_crd(test.client.clone()).await;

    let executor: K8sExecutor = K8sExecutor::try_default(test_ns.to_string()).await.unwrap();

    let cr_name = "test-cr";
    let cr = Foo::new(cr_name, FooSpec {});

    executor
        .create_dynamic_object(
            GroupVersionKind::gvk("newrelic.com", "v1", "Foo"),
            serde_yaml::to_string(&cr).unwrap().as_str(),
        )
        .await
        .unwrap();

    let api: Api<Foo> = Api::namespaced(test.client.clone(), &test_ns);

    // Asserts that the CR has been created in the namespace
    api.get(cr_name).await.unwrap();
}

// Example code to replace with real test when added.
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
    use kube::Client;

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
