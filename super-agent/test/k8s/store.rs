use super::common::{block_on, tokio_runtime, K8sEnv};
use k8s_openapi::api::core::v1::ConfigMap;
use kube::Api;
use newrelic_super_agent::k8s::client::SyncK8sClient;
use newrelic_super_agent::k8s::labels::Labels;
use newrelic_super_agent::k8s::store::{
    K8sStore, StoreKey, CM_NAME_PREFIX, STORE_KEY_INSTANCE_ID, STORE_KEY_REMOTE_CONFIG_HASH,
};
use newrelic_super_agent::opamp::hash_repository::{HashRepository, HashRepositoryConfigMap};
use newrelic_super_agent::opamp::instance_id::{
    getter::{InstanceIDGetter, ULIDInstanceIDGetter},
    Identifiers,
};
use newrelic_super_agent::opamp::remote_config_hash::Hash;
use newrelic_super_agent::super_agent::config::AgentID;
use std::sync::Arc;

const AGENT_ID_1: &str = "agent-id-test";
const AGENT_ID_2: &str = "agent-different-id-test";

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_instance_id_store() {
    // This test covers the happy path of ULIDInstanceIDGetter on K8s.
    // It checks that with same AgentID the the Ulid is the same and if different the ULID is different

    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());

    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime(), test_ns.clone()).unwrap());
    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));

    let agent_id_1 = AgentID::new(AGENT_ID_1).unwrap();
    let agent_id_2 = AgentID::new(AGENT_ID_2).unwrap();

    let instance_id_getter =
        ULIDInstanceIDGetter::try_with_identifiers(k8s_store, Identifiers::default()).unwrap();

    let instance_id_created_1 = instance_id_getter.get(&agent_id_1).unwrap();
    let instance_id_1 = instance_id_getter.get(&agent_id_1).unwrap();
    assert_eq!(instance_id_created_1, instance_id_1);

    let instance_id_created_2 = instance_id_getter.get(&agent_id_2).unwrap();
    let instance_id_2 = instance_id_getter.get(&agent_id_2).unwrap();
    assert_eq!(instance_id_created_2, instance_id_2);
    assert_ne!(instance_id_created_1, instance_id_created_2);

    // Check multiple retrievals
    let instance_id_1 = instance_id_getter.get(&agent_id_1).unwrap();
    assert_eq!(instance_id_created_1, instance_id_1);
    let instance_id_2 = instance_id_getter.get(&agent_id_2).unwrap();
    assert_eq!(instance_id_created_2, instance_id_2);

    let cm_client: Api<ConfigMap> =
        Api::<ConfigMap>::namespaced(test.client.clone(), test_ns.as_str());
    assert_agent_cm(&cm_client, &agent_id_1, STORE_KEY_INSTANCE_ID);
    assert_agent_cm(&cm_client, &agent_id_2, STORE_KEY_INSTANCE_ID);
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_hash_repository_config_map() {
    // This test covers the happy path of HashRepositoryConfigMap on K8s.
    // It checks that with same AgentID the Hash is the same and if different the Hash is different

    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());

    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime(), test_ns.clone()).unwrap());
    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));
    let agent_id_1 = AgentID::new(AGENT_ID_1).unwrap();
    let agent_id_2 = AgentID::new(AGENT_ID_2).unwrap();

    let hash_repository = HashRepositoryConfigMap::new(k8s_store);

    assert_eq!(None, hash_repository.get(&agent_id_1).unwrap());

    let hash_1 = Hash::new("hash-test".to_string());
    hash_repository.save(&agent_id_1, &hash_1).unwrap();
    let loaded_hash_1 = hash_repository.get(&agent_id_1).unwrap().unwrap();
    assert_eq!(hash_1, loaded_hash_1);

    let hash2 = Hash::new("hash-test2".to_string());
    hash_repository.save(&agent_id_2, &hash2).unwrap();
    let loaded_hash_2 = hash_repository.get(&agent_id_2).unwrap().unwrap();
    assert_eq!(hash2, loaded_hash_2);

    let cm_client: Api<ConfigMap> =
        Api::<ConfigMap>::namespaced(test.client.clone(), test_ns.as_str());
    assert_agent_cm(&cm_client, &agent_id_1, STORE_KEY_REMOTE_CONFIG_HASH);
    assert_agent_cm(&cm_client, &agent_id_2, STORE_KEY_REMOTE_CONFIG_HASH);
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_multiple_store_entries() {
    // This test exercises all K8s storers that share the same ConfigMap.
    // It checks that all entries are persisted and loaded correctly.

    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());

    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime(), test_ns.clone()).unwrap());
    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));
    let agent_id = AgentID::new(AGENT_ID_1).unwrap();

    // Persisters sharing the ConfigMap
    let hash_repository = HashRepositoryConfigMap::new(k8s_store.clone());
    let instance_id_getter =
        ULIDInstanceIDGetter::try_with_identifiers(k8s_store.clone(), Identifiers::default())
            .unwrap();

    // Add entries to from all persisters
    let hash = Hash::new("hash-test".to_string());
    hash_repository.save(&agent_id, &hash).unwrap();
    let instance_id_created = instance_id_getter.get(&agent_id).unwrap();

    // Assert from loaded entries
    assert_eq!(Some(hash), hash_repository.get(&agent_id).unwrap());
    assert_eq!(
        instance_id_created,
        instance_id_getter.get(&agent_id).unwrap()
    );

    let cm_client: Api<ConfigMap> =
        Api::<ConfigMap>::namespaced(test.client.clone(), test_ns.as_str());
    assert_agent_cm(&cm_client, &agent_id, STORE_KEY_REMOTE_CONFIG_HASH);
    assert_agent_cm(&cm_client, &agent_id, STORE_KEY_INSTANCE_ID);
}

fn assert_agent_cm(cm_client: &Api<ConfigMap>, agent_id: &AgentID, store_key: &StoreKey) {
    let cm_name = format!("{}{}", CM_NAME_PREFIX, agent_id);
    let cm = block_on(cm_client.get(&cm_name));
    assert!(cm.is_ok());
    let cm_un = cm.unwrap();
    assert!(cm_un.data.is_some());
    assert!(cm_un.data.unwrap().contains_key(store_key));
    assert_eq!(
        cm_un.metadata.labels,
        Some(Labels::new(agent_id).get()),
        "Expect to have default SA labels"
    );
}
