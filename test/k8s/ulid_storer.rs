use crate::common::K8sEnv;
use newrelic_super_agent::opamp::instance_id::{
    self,
    getter::{InstanceIDGetter, ULIDInstanceIDGetter},
};

const AGENT_ID_TEST: &str = "agent-id-test";
const AGENT_DIFFERENT_ID_TEST: &str = "agent-different-id-test";
const AGENT_INVALID_ID_TEST: &str = "agent-invalid-#$^%&%*^&(";

// tokio test runs with 1 thread by default causing deadlock when executing `block_on` code during test helper drop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs k8s cluster"]
async fn k8s_ulid_persister() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let instance_id_getter =
        ULIDInstanceIDGetter::try_default::<instance_id::K8sIdentifiersRetriever>(test_ns.as_str())
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
}
