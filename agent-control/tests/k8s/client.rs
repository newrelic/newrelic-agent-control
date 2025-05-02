use super::tools::{
    k8s_env::K8sEnv,
    test_crd::{Foo, FooSpec, create_foo_cr, foo_type_meta, get_dynamic_api_foo},
};

use crate::k8s::tools::test_crd::{build_dynamic_object, create_crd, delete_crd};
use assert_matches::assert_matches;
use kube::core::DynamicObject;
use kube::{
    CustomResource,
    api::{Api, DeleteParams, TypeMeta},
};
use kube::{CustomResourceExt, ResourceExt};
use newrelic_agent_control::k8s::Error::MissingAPIResource;
use newrelic_agent_control::k8s::{Error, client::AsyncK8sClient};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

const TEST_LABEL_KEY: &str = "key";
const TEST_LABEL_VALUE: &str = "value";

// tokio test runs with 1 thread by default causing deadlock when executing `block_on` code during test helper drop.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_missing_namespace_creation_fail() {
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
async fn k8s_dynamic_resource_has_changed_secret() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let secret_name = "secret-name";

    let k8s_client: AsyncK8sClient = AsyncK8sClient::try_new(test_ns.to_string()).await.unwrap();

    let secret_type_meta = TypeMeta {
        api_version: "v1".into(),
        kind: "Secret".into(),
    };

    let secret = build_dynamic_object(
        secret_type_meta.clone(),
        secret_name.to_string(),
        serde_json::json!({"stringData": {"some-key": "some value"}}),
    );

    // Create the secret in the cluster and wait some time to be sure it is already and the reflector gets it.
    k8s_client
        .dynamic_object_managers()
        .apply(&secret)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Get the secret from the cluster (the content of `string_data` is encoded into `data`)
    let stored_secret = k8s_client
        .dynamic_object_managers()
        .get(&secret_type_meta, secret_name)
        .await
        .unwrap()
        .expect("The secret should exist");

    assert!(
        !k8s_client
            .dynamic_object_managers()
            .has_changed(&secret)
            .await
            .unwrap(),
        "No changes are expected when comparing to the secret from manifest"
    );

    assert!(
        !k8s_client
            .dynamic_object_managers()
            .has_changed(&stored_secret)
            .await
            .unwrap(),
        "No changes are expected when comparing to the secret already stored"
    );

    let new_content_secret = build_dynamic_object(
        secret_type_meta.clone(),
        secret_name.to_string(),
        serde_json::json!({"stringData": {"some-key": "a different value"}}),
    );

    assert!(
        k8s_client
            .dynamic_object_managers()
            .has_changed(&new_content_secret)
            .await
            .unwrap(),
        "Changes are expected when comparing new values"
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

// Test that the reflectors of dynamic objects are consistent when the CRD is removed.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_remove_crd_after_dynamic_resource_initialized() {
    let mut k8s = K8sEnv::new().await;
    let test_ns = k8s.test_namespace().await;
    // custom CRD defined for this test only.
    #[derive(Default, CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
    #[kube(
        group = "newrelic.com",
        version = "v1",
        kind = "ClientTest",
        namespaced
    )]
    pub struct ClientTestSpec {
        pub data: String,
    }
    delete_crd(k8s.client.clone(), ClientTest::crd())
        .await
        .expect_err("CRD deleted, testing environment was not clean, re-run the test");

    create_crd(k8s.client.clone(), ClientTest::crd())
        .await
        .expect("Error creating the Bar CRD");

    let k8s_client = AsyncK8sClient::try_new(test_ns.to_string()).await.unwrap();

    let cr = ClientTest::new(
        "test-cr",
        ClientTestSpec {
            data: "on_create".to_string(),
        },
    );

    let dynamic_object = serde_yaml::from_value(serde_yaml::to_value(cr).unwrap()).unwrap();

    k8s_client
        .dynamic_object_managers()
        .apply(&dynamic_object)
        .await
        .unwrap();

    delete_crd(k8s.client.clone(), ClientTest::crd())
        .await
        .unwrap();

    //wait for the reflector to be updated
    tokio::time::sleep(Duration::from_secs(5)).await;

    assert_matches!(
        k8s_client
            .dynamic_object_managers()
            .get(
                &dynamic_object.types.clone().unwrap(),
                &dynamic_object.name_unchecked(),
            )
            .await,
        Err(MissingAPIResource(_)),
        "CR was removed client should not find it"
    );

    assert_matches!(
        k8s_client
            .dynamic_object_managers()
            .apply(&dynamic_object)
            .await,
        Err(MissingAPIResource(_)),
        "CRD was removed, client should not be able to create a new object"
    );

    // re-create the CRD
    create_crd(k8s.client.clone(), ClientTest::crd())
        .await
        .expect("Error creating the Bar CRD");

    // wait for the CRD to be created
    tokio::time::sleep(Duration::from_secs(1)).await;

    let new_cr = ClientTest::new(
        "other",
        ClientTestSpec {
            data: "on_create".to_string(),
        },
    );
    let new_dyn_object = serde_yaml::from_value(serde_yaml::to_value(new_cr).unwrap()).unwrap();

    k8s_client
        .dynamic_object_managers()
        .apply(&new_dyn_object)
        .await
        .expect("CRD was re-created, client should be able to create a new object");

    // wait for the reflector to be updated
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Old removed CR should not be found
    assert!(
        k8s_client
            .dynamic_object_managers()
            .get(
                &dynamic_object.types.clone().unwrap(),
                &dynamic_object.name_unchecked(),
            )
            .await
            .unwrap()
            .is_none()
    );
    // New CR should be found
    assert!(
        k8s_client
            .dynamic_object_managers()
            .get(
                &new_dyn_object.types.clone().unwrap(),
                &new_dyn_object.name_unchecked(),
            )
            .await
            .unwrap()
            .is_some()
    );

    // clean up
    delete_crd(k8s.client.clone(), ClientTest::crd())
        .await
        .unwrap();
}
