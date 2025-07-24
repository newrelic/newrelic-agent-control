use super::tools::{
    k8s_env::K8sEnv,
    test_crd::{Foo, FooSpec, create_foo_cr, foo_type_meta, get_dynamic_api_foo},
};
use crate::common::retry::retry;
use crate::common::runtime::{block_on, tokio_runtime};
use crate::k8s::tools::test_crd::{
    build_dynamic_object, create_crd, delete_crd, get_foo_dynamic_object,
};
use assert_matches::assert_matches;
use k8s_openapi::api::apps::v1::{StatefulSet, StatefulSetSpec};
use k8s_openapi::api::core::v1::{ConfigMap, PodTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::api::PostParams;
use kube::core::DynamicObject;
use kube::{
    CustomResource,
    api::{Api, DeleteParams, TypeMeta},
};
use kube::{CustomResourceExt, ResourceExt};
use newrelic_agent_control::k8s::Error::MissingAPIResource;
use newrelic_agent_control::k8s::client::SyncK8sClient;
use newrelic_agent_control::k8s::utils::get_type_meta;
use newrelic_agent_control::k8s::{Error, client::AsyncK8sClient};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const TEST_LABEL_KEY: &str = "key";
const TEST_LABEL_VALUE: &str = "value";

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_create_statefulset_retrieve_dynamic_via_reflector_and_trasform_it_back() {
    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());

    let api: Api<StatefulSet> = Api::<StatefulSet>::namespaced(test.client.clone(), &test_ns);

    block_on(api.create(
        &PostParams::default(),
        &StatefulSet {
            metadata: ObjectMeta {
                name: Some("test-statefulset".to_string()),
                namespace: Some(test_ns.clone()),
                ..Default::default()
            },
            spec: Some(StatefulSetSpec {
                service_name: Some("test-service".to_string()),
                replicas: Some(1),
                selector: LabelSelector {
                    match_labels: Some([("app".to_string(), "test".to_string())].into()),
                    ..Default::default()
                },
                template: PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        labels: Some([("app".to_string(), "test".to_string())].into()),
                        ..Default::default()
                    }),
                    spec: None,
                },
                ..Default::default()
            }),
            status: None,
        },
    ))
    .unwrap();

    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime()).unwrap());

    retry(60, Duration::from_secs(1), || {
        if k8s_client.list_stateful_set(&test_ns).unwrap().len() == 1 {
            return Ok(());
        }
        Err("StatefulSet should exist after creation".into())
    });
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_manage_dynamic_resource_namespace_does_not_exist() {
    // forces the client to leverage the kubeconfig file
    let _ = K8sEnv::new().await;

    let name_1 = "test-cr-1";
    let test_ns_1 = "this-does-not-exist";
    let cr_1 = get_foo_dynamic_object(name_1.to_string(), test_ns_1.to_string());
    let k8s_client = AsyncK8sClient::try_new().await.unwrap();

    assert!(
        k8s_client
            .apply_dynamic_object(&cr_1)
            .await
            .unwrap_err()
            .to_string()
            .contains("not found"),
        "should not be able to create object in a non-existing namespace"
    );
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_manage_dynamic_resource_multiple_namespaces() {
    let mut test = block_on(K8sEnv::new());
    let test_ns_1 = block_on(test.test_namespace());
    let test_ns_2 = block_on(test.test_namespace());

    let name_1 = "test-cr-1";
    let name_2 = "test-cr-2";

    let cr_1 = get_foo_dynamic_object(name_1.to_string(), test_ns_1.clone());
    let cr_2 = get_foo_dynamic_object(name_2.to_string(), test_ns_2.clone());
    let tm = get_type_meta(&cr_1).unwrap();

    let k8s_client = block_on(AsyncK8sClient::try_new()).unwrap();
    check_number_of_dynamic_objects(&k8s_client, &tm, 0, &test_ns_1);

    block_on(k8s_client.apply_dynamic_object(&cr_1)).unwrap();
    block_on(k8s_client.apply_dynamic_object(&cr_2)).unwrap();

    let k8s_client = block_on(AsyncK8sClient::try_new()).unwrap();
    check_number_of_dynamic_objects(&k8s_client, &tm, 1, &test_ns_1);
    check_number_of_dynamic_objects(&k8s_client, &tm, 1, &test_ns_2);

    block_on(k8s_client.delete_dynamic_object(&tm, name_1, &test_ns_2)).unwrap();
    // No object should be deleted in the first namespace
    check_number_of_dynamic_objects(&k8s_client, &tm, 1, &test_ns_1);

    block_on(k8s_client.delete_dynamic_object(&tm, name_1, &test_ns_1)).unwrap();
    check_number_of_dynamic_objects(&k8s_client, &tm, 0, &test_ns_1);
}

fn check_number_of_dynamic_objects(
    k8s_client: &AsyncK8sClient,
    tm: &TypeMeta,
    number: usize,
    ns: &str,
) {
    retry(60, Duration::from_secs(1), || {
        if block_on(k8s_client.list_dynamic_objects(tm, ns))
            .unwrap()
            .len()
            == number
        {
            return Ok(());
        }
        Err(format!("{number} object should exist after creation").into())
    });
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_create_dynamic_resource() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let name = "test-cr";
    let obj = get_foo_dynamic_object(name.to_string(), test_ns.to_string());

    let k8s_client = AsyncK8sClient::try_new().await.unwrap();

    k8s_client.apply_dynamic_object(&obj).await.unwrap();

    // Assert that object has been created.
    let api: Api<Foo> = Api::namespaced(test.client.clone(), &test_ns);
    let result = api.get(name).await.expect("fail creating the cr");
    assert_eq!(String::from("test"), result.spec.data);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_get_dynamic_resource() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "get-test";

    let k8s_client = AsyncK8sClient::try_new().await.unwrap();

    assert!(
        k8s_client
            .get_dynamic_object(&foo_type_meta(), cr_name, &test_ns)
            .await
            .unwrap()
            .is_none(),
        "Get doesn't find any object before creation"
    );

    create_foo_cr(test.client.to_owned(), &test_ns, cr_name, None, None).await;

    let cr = k8s_client
        .get_dynamic_object(&foo_type_meta(), cr_name, &test_ns)
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
            .get_dynamic_object(&foo_type_meta(), cr_name, &test_ns)
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

    let k8s_client = AsyncK8sClient::try_new().await.unwrap();

    assert!(
        k8s_client
            .get_dynamic_object(&foo_type_meta(), cr_name, &test_ns)
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
        .get_dynamic_object(&foo_type_meta(), cr_name, &test_ns)
        .await
        .unwrap()
        .expect("The object should be found after creation");

    assert!(
        !k8s_client
            .has_dynamic_object_changed(cr.as_ref())
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
            .has_dynamic_object_changed(&cr_labels_modified)
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
            .has_dynamic_object_changed(&cr_specs_modified)
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
            .has_dynamic_object_changed(&cr_specs_modified)
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

    let k8s_client = AsyncK8sClient::try_new().await.unwrap();

    let secret_type_meta = TypeMeta {
        api_version: "v1".into(),
        kind: "Secret".into(),
    };

    let secret = build_dynamic_object(
        secret_type_meta.clone(),
        secret_name.to_string(),
        test_ns.to_string(),
        serde_json::json!({"stringData": {"some-key": "some value"}}),
    );

    // Create the secret in the cluster and wait some time to be sure it is already and the reflector gets it.
    k8s_client.apply_dynamic_object(&secret).await.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Get the secret from the cluster (the content of `string_data` is encoded into `data`)
    let stored_secret = k8s_client
        .get_dynamic_object(&secret_type_meta, secret_name, &test_ns)
        .await
        .unwrap()
        .expect("The secret should exist");

    assert!(
        !k8s_client
            .has_dynamic_object_changed(&secret)
            .await
            .unwrap(),
        "No changes are expected when comparing to the secret from manifest"
    );

    assert!(
        !k8s_client
            .has_dynamic_object_changed(&stored_secret)
            .await
            .unwrap(),
        "No changes are expected when comparing to the secret already stored"
    );

    let new_content_secret = build_dynamic_object(
        secret_type_meta.clone(),
        secret_name.to_string(),
        test_ns.to_string(),
        serde_json::json!({"stringData": {"some-key": "a different value"}}),
    );

    assert!(
        k8s_client
            .has_dynamic_object_changed(&new_content_secret)
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

    let k8s_client = AsyncK8sClient::try_new().await.unwrap();

    k8s_client
        .delete_dynamic_object(&foo_type_meta(), cr_name, &test_ns)
        .await
        .expect("Delete should not fail");

    let api: Api<Foo> = Api::namespaced(test.client.to_owned(), test_ns.as_str());
    api.get(cr_name).await.expect_err("fail removing the cr");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_update_dynamic_resource() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "update-test";
    let mut cr = create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        cr_name,
        None,
        None,
    )
    .await;

    cr.spec.data = "updated".to_string();
    let obj: DynamicObject =
        serde_yaml::from_str(serde_yaml::to_string(&cr).unwrap().as_str()).unwrap();

    let k8s_client = AsyncK8sClient::try_new().await.unwrap();
    k8s_client
        .apply_dynamic_object(&obj)
        .await
        .expect("Apply should not fail");

    let api: Api<Foo> = Api::namespaced(test.client.to_owned(), test_ns.as_str());
    let result = api.get(cr_name).await.expect("The CR should exist");
    assert_eq!(String::from("updated"), result.spec.data);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_update_dynamic_resource_metadata() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let cr_name = "update-test";
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
    let k8s_client = AsyncK8sClient::try_new().await.unwrap();
    k8s_client
        .apply_dynamic_object(&obj)
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
async fn k8s_patch_dynamic_resource() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;
    let cr_name = "patch-test";

    let k8s_client = AsyncK8sClient::try_new().await.unwrap();
    assert!(
        k8s_client
            .patch_dynamic_object(
                &foo_type_meta(),
                cr_name,
                &test_ns,
                serde_json::json!({
                    "spec": {
                        "data": "patched"
                    }
                }),
            )
            .await
            .is_err()
    );

    let cr = Foo {
        metadata: ObjectMeta {
            name: Some(cr_name.to_string()),
            namespace: Some(test_ns.to_string()),
            ..Default::default()
        },
        spec: FooSpec {
            data: String::from("created"),
        },
    };
    let obj: DynamicObject =
        serde_yaml::from_str(serde_yaml::to_string(&cr).unwrap().as_str()).unwrap();
    k8s_client.apply_dynamic_object(&obj).await.unwrap();

    let api: Api<Foo> = Api::namespaced(test.client.to_owned(), test_ns.as_str());
    let foo = api.get(cr_name).await.unwrap();
    assert_eq!(foo.spec.data, "created");

    let _ = k8s_client
        .patch_dynamic_object(
            &foo_type_meta(),
            cr_name,
            &test_ns,
            serde_json::json!({
                "spec": {
                    "data": "patched"
                }
            }),
        )
        .await
        .unwrap();
    let foo = api.get(cr_name).await.expect("The CR should exist");
    assert_eq!(foo.spec.data, "patched");
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
        metadata: ObjectMeta {
            name: Some(cr_name.to_string()),
            namespace: Some(test_ns.clone()),
            ..Default::default()
        },
        data: Default::default(),
    };

    let k8s_client = AsyncK8sClient::try_new().await.unwrap();

    assert_matches!(
        k8s_client
            .get_dynamic_object(&type_meta, cr_name, &test_ns)
            .await
            .unwrap_err(),
        Error::MissingAPIResource(_)
    );
    assert_matches!(
        k8s_client
            .apply_dynamic_object(&dynamic_object)
            .await
            .unwrap_err(),
        Error::MissingAPIResource(_)
    );
    assert_matches!(
        k8s_client
            .apply_dynamic_object_if_changed(&dynamic_object)
            .await
            .unwrap_err(),
        Error::MissingAPIResource(_)
    );
    assert_matches!(
        k8s_client
            .delete_dynamic_object(&type_meta, cr_name, &test_ns)
            .await
            .unwrap_err(),
        Error::MissingAPIResource(_)
    );
    assert_matches!(
        k8s_client
            .has_dynamic_object_changed(&dynamic_object)
            .await
            .unwrap_err(),
        Error::MissingAPIResource(_)
    );
    assert_matches!(
        k8s_client
            .list_dynamic_objects(&type_meta, &test_ns)
            .await
            .unwrap_err(),
        Error::MissingAPIResource(_)
    );
}

// Test that the reflectors of dynamic objects are consistent when the CRD is removed.
#[test]
#[ignore = "needs k8s cluster"]
fn k8s_remove_crd_after_dynamic_resource_initialized() {
    let mut k8s = block_on(K8sEnv::new());
    let test_ns = block_on(k8s.test_namespace());
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
    block_on(delete_crd(k8s.client.clone(), ClientTest::crd()))
        .expect_err("CRD deleted, testing environment was not clean, re-run the test");

    block_on(create_crd(k8s.client.clone(), ClientTest::crd()))
        .expect("Error creating the Bar CRD");

    let k8s_client = SyncK8sClient::try_new(tokio_runtime()).unwrap();

    let cr = ClientTest {
        metadata: ObjectMeta {
            name: Some("test-cr".to_string()),
            namespace: Some(test_ns.to_string()),
            ..Default::default()
        },
        spec: ClientTestSpec {
            data: "on_create".to_string(),
        },
    };

    let dynamic_object = serde_yaml::from_value(serde_yaml::to_value(cr).unwrap()).unwrap();

    // Applying the CRD object to the cluster could take some time, so we retry a few times.
    retry(10, Duration::from_secs(1), || {
        k8s_client
            .apply_dynamic_object(&dynamic_object)
            .map_err(|e| e.into())
    });

    block_on(delete_crd(k8s.client.clone(), ClientTest::crd())).unwrap();

    //wait for the reflector to be updated
    std::thread::sleep(Duration::from_secs(5));

    assert_matches!(
        k8s_client.get_dynamic_object(
            &dynamic_object.types.clone().unwrap(),
            &dynamic_object.name_unchecked(),
            &test_ns,
        ),
        Err(MissingAPIResource(_)),
        "CR was removed client should not find it"
    );

    assert_matches!(
        k8s_client.apply_dynamic_object(&dynamic_object),
        Err(MissingAPIResource(_)),
        "CRD was removed, client should not be able to create a new object"
    );

    // re-create the CRD
    block_on(create_crd(k8s.client.clone(), ClientTest::crd()))
        .expect("Error creating the Bar CRD");

    // wait for the CRD to be created
    std::thread::sleep(Duration::from_secs(1));

    let new_cr = ClientTest {
        metadata: ObjectMeta {
            name: Some("other".to_string()),
            namespace: Some(test_ns.to_string()),
            ..Default::default()
        },
        spec: ClientTestSpec {
            data: "on_create".to_string(),
        },
    };

    let new_dyn_object = serde_yaml::from_value(serde_yaml::to_value(new_cr).unwrap()).unwrap();

    k8s_client
        .apply_dynamic_object(&new_dyn_object)
        .expect("CRD was re-created, client should be able to create a new object");

    // wait for the reflector to be updated
    std::thread::sleep(Duration::from_secs(5));

    // Old removed CR should not be found
    assert!(
        k8s_client
            .get_dynamic_object(
                &dynamic_object.types.clone().unwrap(),
                &dynamic_object.name_unchecked(),
                &test_ns,
            )
            .unwrap()
            .is_none()
    );
    // New CR should be found
    assert!(
        k8s_client
            .get_dynamic_object(
                &new_dyn_object.types.clone().unwrap(),
                &new_dyn_object.name_unchecked(),
                &test_ns,
            )
            .unwrap()
            .is_some()
    );

    // clean up
    block_on(delete_crd(k8s.client.clone(), ClientTest::crd())).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
// this test is related to the workaround in place to avoid https://github.com/kube-rs/kube/issues/1796
// Once that it is properly fixed, this test can be removed
async fn k8s_client_does_not_hang_in_case_of_incomplete_message() {
    let now = SystemTime::now();
    // when the logs are enabled, the test hangs less often and it is no longer valid
    let mut test = K8sEnv::new_without_logs().await;
    let test_ns_1 = test.test_namespace().await;
    let name = "test-cm";

    let cr = ConfigMap {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(test_ns_1),
            ..Default::default()
        },
        ..Default::default()
    };

    let obj: DynamicObject =
        serde_yaml::from_str(serde_yaml::to_string(&cr).unwrap().as_str()).unwrap();

    for i in 1..100 {
        println!(
            "New Try {:}: milliseconds {:}",
            i,
            SystemTime::now().duration_since(now).unwrap().as_millis()
        );

        let client = AsyncK8sClient::try_new()
            .await
            .expect("fail to create client");

        client.apply_dynamic_object(&obj).await.unwrap();
    }

    if SystemTime::now().duration_since(now).unwrap() > Duration::from_secs(10) {
        panic!("The test took too long to complete, it should not hang");
    }
}
