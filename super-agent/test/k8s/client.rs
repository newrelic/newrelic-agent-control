use crate::common::{create_test_cr, k8s_env};
use k8s_test_env::foo_crd::{foo_type_meta, get_dynamic_api_foo, Foo, FooSpec};
use kube::api::{Api, DeleteParams};
use kube::core::DynamicObject;
use newrelic_super_agent::k8s::client::AsyncK8sClient;
use serde_json::Value;
use std::time::Duration;

const TEST_LABEL_KEY: &str = "key";
const TEST_LABEL_VALUE: &str = "value";

// tokio test runs with 1 thread by default causing deadlock when executing `block_on` code during test helper drop.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_client_creation_fail() {
    let test_ns = "test-not-existing-namespace";
    assert!(AsyncK8sClient::try_new(test_ns.to_string()).await.is_err());
}

// tokio test runs with 1 thread by default causing deadlock when executing `block_on` code during test helper drop.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_create_dynamic_resource() {
    let mut test = k8s_env().await;
    let test_ns = test.test_namespace().await;

    let name = "test-cr";
    let cr = serde_yaml::to_string(&Foo::new(
        name,
        FooSpec {
            data: String::from("on_create"),
        },
    ))
    .unwrap();
    let obj: DynamicObject = serde_yaml::from_str(cr.as_str()).unwrap();

    let k8s_client: AsyncK8sClient =
        AsyncK8sClient::try_new_with_reflectors(test_ns.to_string(), vec![foo_type_meta()])
            .await
            .unwrap();

    k8s_client.apply_dynamic_object(&obj).await.unwrap();

    // Assert that object has been created.
    let api: Api<Foo> = Api::namespaced(test.client.clone(), &test_ns);
    let result = api.get(name).await.expect("fail creating the cr");
    assert_eq!(String::from("on_create"), result.spec.data);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_get_dynamic_resource() {
    let mut test = k8s_env().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "get-test";

    let k8s_client: AsyncK8sClient =
        AsyncK8sClient::try_new_with_reflectors(test_ns.to_string(), vec![foo_type_meta()])
            .await
            .unwrap();

    // get doesn't find any object before creation.
    assert!(k8s_client
        .get_dynamic_object(&foo_type_meta(), cr_name)
        .await
        .unwrap()
        .is_none());

    create_test_cr(test.client.to_owned(), test_ns.as_str(), cr_name).await;

    // the object is found after creation.
    let cr = k8s_client
        .get_dynamic_object(&foo_type_meta(), cr_name)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(cr.metadata.to_owned().name.unwrap().as_str(), cr_name);

    Api::<Foo>::namespaced(test.client.to_owned(), &test_ns)
        .delete(cr_name, &DeleteParams::default())
        .await
        .unwrap();

    // we should give the time to the cache to be updated for sure
    tokio::time::sleep(Duration::from_secs(1)).await;

    // get doesn't find any object after deletion.
    assert!(k8s_client
        .get_dynamic_object(&foo_type_meta(), cr_name)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_dynamic_resource_has_changed() {
    let mut test = k8s_env().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "has-changed-test";

    let k8s_client: AsyncK8sClient =
        AsyncK8sClient::try_new_with_reflectors(test_ns.to_string(), vec![foo_type_meta()])
            .await
            .unwrap();

    // get doesn't find any object before creation.
    assert!(k8s_client
        .get_dynamic_object(&foo_type_meta(), cr_name)
        .await
        .unwrap()
        .is_none());

    create_test_cr(test.client.to_owned(), test_ns.as_str(), cr_name).await;

    // the object is found after creation.
    let cr = k8s_client
        .get_dynamic_object(&foo_type_meta(), cr_name)
        .await
        .unwrap()
        .unwrap();

    // the object found has not changed
    assert!(!k8s_client
        .has_dynamic_object_changed(cr.as_ref())
        .await
        .unwrap());

    // changing a label
    let mut cr_labels_modified = DynamicObject {
        types: cr.types.clone(),
        metadata: cr.metadata.clone(),
        data: cr.data.clone(),
    };
    cr_labels_modified.metadata.labels = None;
    assert!(k8s_client
        .has_dynamic_object_changed(&cr_labels_modified)
        .await
        .unwrap());

    // changing specs
    let mut cr_specs_modified = DynamicObject {
        types: cr.types.clone(),
        metadata: cr.metadata.clone(),
        data: cr.data.clone(),
    };
    cr_specs_modified.data["spec"] = Value::Bool(false);
    assert!(k8s_client
        .has_dynamic_object_changed(&cr_specs_modified)
        .await
        .unwrap());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_delete_dynamic_resource() {
    let mut test = k8s_env().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "delete-test";
    create_test_cr(test.client.to_owned(), test_ns.as_str(), cr_name).await;

    let k8s_client: AsyncK8sClient =
        AsyncK8sClient::try_new_with_reflectors(test_ns.to_string(), vec![foo_type_meta()])
            .await
            .unwrap();
    k8s_client
        .delete_dynamic_object(foo_type_meta(), cr_name)
        .await
        .unwrap();

    let api: Api<Foo> = Api::namespaced(test.client.to_owned(), test_ns.as_str());
    api.get(cr_name).await.expect_err("fail removing the cr");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_patch_dynamic_resource() {
    let mut test = k8s_env().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "patch-test";
    let mut cr = create_test_cr(test.client.to_owned(), test_ns.as_str(), cr_name).await;

    cr.spec.data = "patched".to_string();
    let obj: DynamicObject =
        serde_yaml::from_str(serde_yaml::to_string(&cr).unwrap().as_str()).unwrap();

    let k8s_client: AsyncK8sClient =
        AsyncK8sClient::try_new_with_reflectors(test_ns.to_string(), vec![foo_type_meta()])
            .await
            .unwrap();
    k8s_client.apply_dynamic_object(&obj).await.unwrap();

    let api: Api<Foo> = Api::namespaced(test.client.to_owned(), test_ns.as_str());
    let result = api.get(cr_name).await.expect("fail creating the cr");
    assert_eq!(String::from("patched"), result.spec.data);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_patch_dynamic_resource_metadata() {
    let mut test = k8s_env().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "patch-test";
    let mut cr = create_test_cr(test.client.to_owned(), test_ns.as_str(), cr_name).await;

    // Adding a label that should be patched
    cr.metadata
        .labels
        .as_mut()
        .unwrap()
        .insert(TEST_LABEL_KEY.to_string(), TEST_LABEL_VALUE.to_string());

    // Changing a second option that will not be patched
    cr.metadata.deletion_grace_period_seconds = Some(99);

    let obj: DynamicObject =
        serde_yaml::from_str(serde_yaml::to_string(&cr).unwrap().as_str()).unwrap();
    let k8s_client: AsyncK8sClient =
        AsyncK8sClient::try_new_with_reflectors(test_ns.to_string(), vec![foo_type_meta()])
            .await
            .unwrap();
    k8s_client.apply_dynamic_object(&obj).await.unwrap();

    let api = get_dynamic_api_foo(test.client.clone(), test_ns).await;
    let result = api.get(cr_name).await.expect("fail creating the cr");
    assert_eq!(
        TEST_LABEL_VALUE.to_string(),
        result
            .metadata
            .labels
            .as_ref()
            .unwrap()
            .get(TEST_LABEL_KEY)
            .unwrap()
            .to_string()
    );
    assert!(result.metadata.deletion_grace_period_seconds.is_none());
}
