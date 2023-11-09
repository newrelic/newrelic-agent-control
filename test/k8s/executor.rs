use crate::common::{create_test_cr, foo_gvk, Foo, FooSpec, K8sCluster, K8sEnv};
use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, DeleteParams};

use newrelic_super_agent::k8s::executor::K8sExecutor;

// tokio test runs with 1 thread by default causing deadlock when executing `block_on` code during test helper drop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs k8s cluster"]
async fn k8s_create_dynamic_resource() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "test-cr";
    let cr = serde_yaml::to_string(&Foo::new(
        cr_name,
        FooSpec {
            data: String::from("on_create"),
        },
    ))
    .unwrap();

    let executor: K8sExecutor = K8sExecutor::try_default(test_ns.to_string()).await.unwrap();
    executor
        .create_dynamic_object(foo_gvk(), cr.as_str())
        .await
        .unwrap();

    // Assert that object has been created.
    let api: Api<Foo> = Api::namespaced(test.client.clone(), &test_ns);
    let result = api.get(cr_name).await.expect("fail creating the cr");
    assert_eq!(String::from("on_create"), result.spec.data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs k8s cluster"]
async fn k8s_get_dynamic_resource() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "get-test";

    let mut executor: K8sExecutor = K8sExecutor::try_default(test_ns.to_string()).await.unwrap();

    // get doesn't find any object before creation.
    assert!(executor
        .get_dynamic_object(foo_gvk(), cr_name)
        .await
        .unwrap()
        .is_none());

    create_test_cr(test.client.to_owned(), test_ns.as_str(), cr_name).await;

    // the object is found after creation.
    let cr = executor
        .get_dynamic_object(foo_gvk(), cr_name)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(cr.metadata.to_owned().name.unwrap().as_str(), cr_name);

    Api::<Foo>::namespaced(test.client.to_owned(), &test_ns)
        .delete(cr_name, &DeleteParams::default())
        .await
        .unwrap();

    // get doesn't find any object after deletion.
    assert!(executor
        .get_dynamic_object(foo_gvk(), cr_name)
        .await
        .unwrap()
        .is_none());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs k8s cluster"]
async fn k8s_delete_dynamic_resource() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "delete-test";
    create_test_cr(test.client.to_owned(), test_ns.as_str(), cr_name).await;

    let executor: K8sExecutor = K8sExecutor::try_default(test_ns.to_string()).await.unwrap();
    executor
        .delete_dynamic_object(foo_gvk(), cr_name)
        .await
        .unwrap();

    let api: Api<Foo> = Api::namespaced(test.client.to_owned(), test_ns.as_str());
    api.get(cr_name).await.expect_err("fail removing the cr");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs k8s cluster"]
async fn k8s_patch_dynamic_resource() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "patch-test";
    create_test_cr(test.client.to_owned(), test_ns.as_str(), cr_name).await;

    let patch = serde_yaml::to_string(&Foo::new(
        cr_name,
        FooSpec {
            data: String::from("patched"),
        },
    ))
    .unwrap();

    let executor: K8sExecutor = K8sExecutor::try_default(test_ns.to_string()).await.unwrap();
    executor
        .patch_dynamic_object(foo_gvk(), cr_name, patch.as_str())
        .await
        .unwrap();

    let api: Api<Foo> = Api::namespaced(test.client.to_owned(), test_ns.as_str());
    let result = api.get(cr_name).await.expect("fail creating the cr");
    assert_eq!(String::from("patched"), result.spec.data);
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
