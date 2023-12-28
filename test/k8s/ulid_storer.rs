use crate::common::K8sEnv;
use k8s_openapi::api::core::v1::ConfigMap;
use kube::Api;
use newrelic_super_agent::config::super_agent_configs::AgentID;
use newrelic_super_agent::k8s::executor::K8sExecutor;
use newrelic_super_agent::k8s::labels::Labels;
use newrelic_super_agent::opamp::instance_id::{
    getter::{InstanceIDGetter, ULIDInstanceIDGetter},
    Identifiers, CM_KEY,
};
use std::sync::Arc;

const AGENT_ID_TEST: &str = "agent-id-test";
const AGENT_DIFFERENT_ID_TEST: &str = "agent-different-id-test";

// tokio test runs with 1 thread by default causing deadlock when executing `block_on` code during test helper drop.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_ulid_persister() {
    // This tests cover the happy path of ULIDInstanceIDGetter on K8s.
    // It checks that with same AgentID the the Ulid is the same and if different the ULID is different

    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;
    let executor = Arc::new(K8sExecutor::try_new(test_ns.clone()).await.unwrap());
    let agent_id = AgentID::new(AGENT_ID_TEST).unwrap();
    let another_agent_id = AgentID::new(AGENT_DIFFERENT_ID_TEST).unwrap();

    let instance_id_getter =
        ULIDInstanceIDGetter::try_with_identifiers(executor, Identifiers::default())
            .await
            .unwrap();

    let value = instance_id_getter.get(&agent_id).unwrap();
    let value2 = instance_id_getter.get(&agent_id).unwrap();
    assert_eq!(value, value2);

    let value_different = instance_id_getter.get(&another_agent_id).unwrap();
    assert_ne!(value, value_different);

    let value4 = instance_id_getter.get(&agent_id).unwrap();
    assert_eq!(value, value4);

    let value_different2 = instance_id_getter.get(&another_agent_id).unwrap();
    assert_eq!(value_different, value_different2);

    // Verify also that the status of the cluster is the expected one
    let cm_client: Api<ConfigMap> =
        Api::<ConfigMap>::namespaced(test.client.clone(), test_ns.clone().as_str());

    let cm = cm_client.get("ulid-data-agent-id-test").await;
    assert!(cm.is_ok());
    let cm_un = cm.unwrap();
    assert!(cm_un.data.is_some());
    assert!(cm_un.data.unwrap().contains_key(CM_KEY));
    assert_eq!(
        cm_un.metadata.labels,
        Some(Labels::new(&agent_id).get()),
        "Expect to have default SA labels"
    );

    let cm = cm_client.get("ulid-data-agent-different-id-test").await;
    assert!(cm.is_ok());
    let cm_un = cm.unwrap();
    assert!(cm_un.data.is_some());
    assert!(cm_un.data.unwrap().contains_key(CM_KEY));
    assert_eq!(
        cm_un.metadata.labels,
        Some(Labels::new(&another_agent_id).get()),
        "Expect to have default SA labels"
    );
}
