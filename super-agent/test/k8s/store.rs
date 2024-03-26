use super::common::{block_on, create_mock_config_maps, tokio_runtime, K8sEnv};
use k8s_openapi::api::core::v1::ConfigMap;
use kube::Api;
use newrelic_super_agent::agent_type::agent_metadata::AgentMetadata;
use newrelic_super_agent::agent_type::agent_values::AgentValues;
use newrelic_super_agent::agent_type::definition::{AgentType, VariableTree};
use newrelic_super_agent::agent_type::runtime_config::Runtime;
use newrelic_super_agent::k8s::client::SyncK8sClient;
use newrelic_super_agent::k8s::labels::Labels;
use newrelic_super_agent::k8s::store::{
    K8sStore, StoreKey, CM_NAME_OPAMP_DATA_PREFIX, STORE_KEY_INSTANCE_ID,
    STORE_KEY_LOCAL_DATA_CONFIG, STORE_KEY_OPAMP_DATA_CONFIG_HASH,
};
use newrelic_super_agent::opamp::hash_repository::{HashRepository, HashRepositoryConfigMap};
use newrelic_super_agent::opamp::instance_id::{
    getter::{InstanceIDGetter, ULIDInstanceIDGetter},
    Identifiers,
};
use newrelic_super_agent::opamp::remote_config_hash::Hash;
use newrelic_super_agent::sub_agent::values::values_repository::ValuesRepository;
use newrelic_super_agent::sub_agent::values::ValuesRepositoryConfigMap;
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
    assert_agent_cm(&cm_client, &agent_id_1, STORE_KEY_OPAMP_DATA_CONFIG_HASH);
    assert_agent_cm(&cm_client, &agent_id_2, STORE_KEY_OPAMP_DATA_CONFIG_HASH);
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_value_repository_config_map() {
    // This test covers the happy path of ValuesRepositoryConfigMap on K8s.

    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());

    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime(), test_ns.clone()).unwrap());
    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));
    let agent_id_1 = AgentID::new(AGENT_ID_1).unwrap();
    let agent_id_2 = AgentID::new(AGENT_ID_2).unwrap();

    let agent_type = AgentType::new(
        AgentMetadata::default(),
        VariableTree::default(),
        Runtime::default(),
    );

    let mut value_repository = ValuesRepositoryConfigMap::new(k8s_store);
    let default_values = AgentValues::default();

    // without values the default is expected
    let res = value_repository.load(&agent_id_1, &agent_type);
    assert_eq!(res.unwrap(), default_values);

    // with local values we expect some data
    block_on(create_mock_config_maps(
        test.client.clone(),
        test_ns.as_str(),
        format!("local-data-{}", AGENT_ID_1).as_str(),
        STORE_KEY_LOCAL_DATA_CONFIG,
    ));
    let local_values = AgentValues::try_from("test: 1".to_string()).unwrap();
    let res = value_repository.load(&agent_id_1, &agent_type);

    assert_eq!(res.unwrap(), local_values);

    // with remote data we expect we get local without remote
    let remote_values = AgentValues::try_from("test: 3".to_string()).unwrap();
    value_repository
        .store_remote(&agent_id_1, &remote_values)
        .unwrap();
    let res = value_repository.load(&agent_id_1, &agent_type);
    assert_eq!(res.unwrap(), local_values);

    // Once we have remote enabled we get remote data
    value_repository = value_repository.with_remote();
    let res = value_repository.load(&agent_id_1, &agent_type);
    assert_eq!(res.unwrap(), remote_values);

    // After deleting remote we expect to get still local data
    value_repository.delete_remote(&agent_id_1).unwrap();
    let res = value_repository.load(&agent_id_1, &agent_type);
    assert_eq!(res.unwrap(), local_values);

    // After saving data for a second agent should not affect the previous one
    // with remote data we expect to ignore local one
    let remote_values_agent_2 = AgentValues::try_from("test: 100".to_string()).unwrap();
    value_repository
        .store_remote(&agent_id_2, &remote_values_agent_2)
        .unwrap();
    let res = value_repository.load(&agent_id_1, &agent_type);
    let res_agent_2 = value_repository.load(&agent_id_2, &agent_type);
    assert_eq!(res.unwrap(), local_values);
    assert_eq!(res_agent_2.unwrap(), remote_values_agent_2);
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
    assert_agent_cm(&cm_client, &agent_id, STORE_KEY_OPAMP_DATA_CONFIG_HASH);
    assert_agent_cm(&cm_client, &agent_id, STORE_KEY_INSTANCE_ID);
}

fn assert_agent_cm(cm_client: &Api<ConfigMap>, agent_id: &AgentID, store_key: &StoreKey) {
    let cm_name = format!("{}{}", CM_NAME_OPAMP_DATA_PREFIX, agent_id);
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
