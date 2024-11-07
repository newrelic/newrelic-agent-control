use super::tools::{
    foo_crd::{create_foo_cr, foo_type_meta, get_dynamic_api_foo, Foo, FooSpec},
    k8s_env::K8sEnv,
};
use assert_matches::assert_matches;
use kube::api::{Api, DeleteParams, TypeMeta};
use kube::core::DynamicObject;
use newrelic_super_agent::k8s::{client::AsyncK8sClient, Error};
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

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_create_dynamic_resource() {
    let mut test = K8sEnv::new().await;
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

    let k8s_client: AsyncK8sClient = AsyncK8sClient::try_new(test_ns.to_string()).await.unwrap();

    k8s_client
        .dynamic_object_managers()
        .apply(&obj)
        .await
        .unwrap();

    // Assert that object has been created.
    let api: Api<Foo> = Api::namespaced(test.client.clone(), &test_ns);
    let result = api.get(name).await.expect("fail creating the cr");
    assert_eq!(String::from("on_create"), result.spec.data);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_get_dynamic_resource() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "get-test";

    let k8s_client: AsyncK8sClient = AsyncK8sClient::try_new(test_ns.to_string()).await.unwrap();

    assert!(
        k8s_client
            .dynamic_object_managers()
            .get(&foo_type_meta(), cr_name)
            .await
            .unwrap()
            .is_none(),
        "Get doesn't find any object before creation"
    );

    create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        cr_name,
        None,
        None,
    )
    .await;

    let cr = k8s_client
        .dynamic_object_managers()
        .get(&foo_type_meta(), cr_name)
        .await
        .unwrap()
        .expect("The object should be found after creation");

    assert_eq!(cr.metadata.to_owned().name.unwrap().as_str(), cr_name);

    Api::<Foo>::namespaced(test.client.to_owned(), &test_ns)
        .delete(cr_name, &DeleteParams::default())
        .await
        .unwrap();

    // we should give the time to the cache to be updated for sure
    tokio::time::sleep(Duration::from_secs(1)).await;

    assert!(
        k8s_client
            .dynamic_object_managers()
            .get(&foo_type_meta(), cr_name)
            .await
            .unwrap()
            .is_none(),
        "Get doesn't find any object after deletion"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_dynamic_resource_has_changed() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "has-changed-test";

    let k8s_client: AsyncK8sClient = AsyncK8sClient::try_new(test_ns.to_string()).await.unwrap();

    assert!(
        k8s_client
            .dynamic_object_managers()
            .get(&foo_type_meta(), cr_name)
            .await
            .unwrap()
            .is_none(),
        "Get doesn't find any object after deletion"
    );

    create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        cr_name,
        None,
        None,
    )
    .await;

    let cr = k8s_client
        .dynamic_object_managers()
        .get(&foo_type_meta(), cr_name)
        .await
        .unwrap()
        .expect("The object should be found after creation");

    assert!(
        !k8s_client
            .dynamic_object_managers()
            .has_changed(cr.as_ref())
            .await
            .unwrap(),
        "The object found has not changed"
    );

    // changing a label
    let mut cr_labels_modified = DynamicObject {
        types: cr.types.clone(),
        metadata: cr.metadata.clone(),
        data: cr.data.clone(),
    };
    cr_labels_modified.metadata.labels = Some([("a".to_string(), "b".to_string())].into());

    assert!(
        k8s_client
            .dynamic_object_managers()
            .has_changed(&cr_labels_modified)
            .await
            .unwrap(),
        "The object found has changed after changing the label"
    );

    // changing specs
    let mut cr_specs_modified = DynamicObject {
        types: cr.types.clone(),
        metadata: cr.metadata.clone(),
        data: cr.data.clone(),
    };
    cr_specs_modified.data["spec"] = Value::Bool(false);

    assert!(
        k8s_client
            .dynamic_object_managers()
            .has_changed(&cr_specs_modified)
            .await
            .unwrap(),
        "The object found has changed after changing the specs"
    );

    // changing annotations
    let mut cr_specs_modified = DynamicObject {
        types: cr.types.clone(),
        metadata: cr.metadata.clone(),
        data: cr.data.clone(),
    };
    cr_specs_modified.metadata.annotations = Some([("c".to_string(), "d".to_string())].into());

    assert!(
        k8s_client
            .dynamic_object_managers()
            .has_changed(&cr_specs_modified)
            .await
            .unwrap(),
        "The object found has changed after changing the specs"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_delete_dynamic_resource() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "delete-test";
    create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        cr_name,
        None,
        None,
    )
    .await;

    let k8s_client: AsyncK8sClient = AsyncK8sClient::try_new(test_ns.to_string()).await.unwrap();

    k8s_client
        .dynamic_object_managers()
        .delete(&foo_type_meta(), cr_name)
        .await
        .expect("Delete should not fail");

    let api: Api<Foo> = Api::namespaced(test.client.to_owned(), test_ns.as_str());
    api.get(cr_name).await.expect_err("fail removing the cr");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_patch_dynamic_resource() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "patch-test";
    let mut cr = create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        cr_name,
        None,
        None,
    )
    .await;

    cr.spec.data = "patched".to_string();
    let obj: DynamicObject =
        serde_yaml::from_str(serde_yaml::to_string(&cr).unwrap().as_str()).unwrap();

    let k8s_client: AsyncK8sClient = AsyncK8sClient::try_new(test_ns.to_string()).await.unwrap();
    k8s_client
        .dynamic_object_managers()
        .apply(&obj)
        .await
        .expect("Apply should not fail");

    let api: Api<Foo> = Api::namespaced(test.client.to_owned(), test_ns.as_str());
    let result = api.get(cr_name).await.expect("The CR should exist");
    assert_eq!(String::from("patched"), result.spec.data);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_patch_dynamic_resource_metadata() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "patch-test";
    let mut cr = create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        cr_name,
        Some([("a".to_string(), "b".to_string())].into()),
        None,
    )
    .await;

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
    let k8s_client: AsyncK8sClient = AsyncK8sClient::try_new(test_ns.to_string()).await.unwrap();
    k8s_client
        .dynamic_object_managers()
        .apply(&obj)
        .await
        .expect("Apply should not fail");

    let api = get_dynamic_api_foo(test.client.clone(), test_ns).await;
    let result = api.get(cr_name).await.expect("The CR should exist");
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

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_dynamic_resource_missing_kind() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let type_meta = TypeMeta {
        api_version: "missing.com/v1".to_string(),
        kind: "ThisKindDoesNotExists".to_string(),
    };
    let cr_name = "test";
    let dynamic_object = DynamicObject {
        types: Some(type_meta.clone()),
        metadata: Default::default(),
        data: Default::default(),
    };

    let k8s_client: AsyncK8sClient = AsyncK8sClient::try_new(test_ns.to_string()).await.unwrap();

    let dynamic_object_managers = k8s_client.dynamic_object_managers();

    assert_matches!(
        dynamic_object_managers
            .get(&type_meta, cr_name)
            .await
            .unwrap_err(),
        Error::MissingAPIResource(_)
    );
    assert_matches!(
        dynamic_object_managers
            .apply(&dynamic_object)
            .await
            .unwrap_err(),
        Error::MissingAPIResource(_)
    );
    assert_matches!(
        dynamic_object_managers
            .apply_if_changed(&dynamic_object)
            .await
            .unwrap_err(),
        Error::MissingAPIResource(_)
    );
    assert_matches!(
        dynamic_object_managers
            .delete(&type_meta, cr_name)
            .await
            .unwrap_err(),
        Error::MissingAPIResource(_)
    );
    assert_matches!(
        dynamic_object_managers
            .has_changed(&dynamic_object)
            .await
            .unwrap_err(),
        Error::MissingAPIResource(_)
    );
    assert_matches!(
        dynamic_object_managers.list(&type_meta).await.unwrap_err(),
        Error::MissingAPIResource(_)
    );
}
