use crate::common::K8sEnv;
use k8s_openapi::api::core::v1::ConfigMap;
use kube::Api;
use newrelic_super_agent::opamp::instance_id::{
    getter::{InstanceIDGetter, ULIDInstanceIDGetter},
    GetterError, Identifiers, CM_KEY,
};

const AGENT_ID_TEST: &str = "agent-id-test";
const AGENT_DIFFERENT_ID_TEST: &str = "agent-different-id-test";
const AGENT_INVALID_ID_TEST: &str = "agent-invalid-#$^%&%*^&(";

// tokio test runs with 1 thread by default causing deadlock when executing `block_on` code during test helper drop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs k8s cluster"]
async fn k8s_ulid_persister() {
    // This tests cover the happy path of ULIDInstanceIDGetter on K8s.
    // It checks that with same AgentID the the Ulid is the same and if different the ULID is different

    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let instance_id_getter =
        ULIDInstanceIDGetter::try_with_identifiers(test_ns.clone(), Identifiers::default())
            .await
            .unwrap();

    let value = instance_id_getter.get(AGENT_ID_TEST).unwrap();
    let value2 = instance_id_getter.get(AGENT_ID_TEST).unwrap();
    assert_eq!(value, value2);

    let value_different = instance_id_getter.get(AGENT_DIFFERENT_ID_TEST).unwrap();
    assert_ne!(value, value_different);

    let value4 = instance_id_getter.get(AGENT_ID_TEST).unwrap();
    assert_eq!(value, value4);

    let value_different2 = instance_id_getter.get(AGENT_DIFFERENT_ID_TEST).unwrap();
    assert_eq!(value_different, value_different2);

    let invalid = instance_id_getter.get(AGENT_INVALID_ID_TEST);
    assert!(invalid.is_err());

    // Verify also that the status of the cluster is the expected one
    let cm_client: Api<ConfigMap> =
        Api::<ConfigMap>::namespaced(test.client.clone(), test_ns.clone().as_str());

    let cm = cm_client.get("ulid-data-agent-id-test").await;
    assert!(cm.is_ok());
    let cm_un = cm.unwrap();
    assert!(cm_un.data.is_some());
    assert!(cm_un.data.unwrap().contains_key(CM_KEY));

    let cm = cm_client.get("ulid-data-agent-different-id-test").await;
    assert!(cm.is_ok());
    let cm_un = cm.unwrap();
    assert!(cm_un.data.is_some());
    assert!(cm_un.data.unwrap().contains_key(CM_KEY));
}

// tokio test runs with 1 thread by default causing deadlock when executing `block_on` code during test helper drop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs k8s cluster"]
async fn k8s_ulid_persister_fail() {
    // This tests cover the error path for the storer. The namespace does not exist, every call is going to fail.

    let test_ns = "test-not-existing-namespace";

    let instance_id_getter =
        ULIDInstanceIDGetter::try_with_identifiers(test_ns.to_string(), Identifiers::default())
            .await
            .unwrap();

    let e = instance_id_getter.get(AGENT_ID_TEST);
    match e.unwrap_err() {
        GetterError::K8sClientInitialization(_) => {
            panic!("this is unexpected, the test should fail contacting the k8s API")
        }
        GetterError::Persisting(_) => return,
    }
}
