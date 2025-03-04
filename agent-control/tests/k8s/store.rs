use crate::common::runtime::{block_on, tokio_runtime};
use crate::k8s::tools::agent_control::{create_config_map, create_local_config_map};
use crate::k8s::tools::k8s_env::K8sEnv;
use k8s_openapi::api::core::v1::ConfigMap;
use kube::Api;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::config_storer::loader_storer::{
    AgentControlDynamicConfigDeleter, AgentControlDynamicConfigLoader,
    AgentControlDynamicConfigStorer,
};
use newrelic_agent_control::agent_control::config_storer::store::AgentControlConfigStore;
use newrelic_agent_control::agent_control::defaults::default_capabilities;
use newrelic_agent_control::k8s::client::SyncK8sClient;
use newrelic_agent_control::k8s::labels::Labels;
use newrelic_agent_control::k8s::store::{
    K8sStore, StoreKey, CM_NAME_LOCAL_DATA_PREFIX, CM_NAME_OPAMP_DATA_PREFIX,
    STORE_KEY_INSTANCE_ID, STORE_KEY_OPAMP_DATA_CONFIG_HASH,
};
use newrelic_agent_control::opamp::hash_repository::k8s::HashRepositoryConfigMap;
use newrelic_agent_control::opamp::hash_repository::HashRepository;
use newrelic_agent_control::opamp::instance_id::{
    getter::{InstanceIDGetter, InstanceIDWithIdentifiersGetter},
    Identifiers,
};
use newrelic_agent_control::opamp::remote_config::hash::Hash;
use newrelic_agent_control::values::yaml_config_repository::{
    load_remote_fallback_local, YAMLConfigRepository,
};
use newrelic_agent_control::{
    values::k8s::YAMLConfigRepositoryConfigMap, values::yaml_config::YAMLConfig,
};
use serde_yaml::from_str;
use std::sync::Arc;

const AGENT_ID_1: &str = "agent-id-test";
const AGENT_ID_2: &str = "agent-different-id-test";

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_instance_id_store() {
    // This test covers the happy path of InstanceIDGetter on K8s.

    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());

    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime(), test_ns.clone()).unwrap());
    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));

    let agent_id_1 = AgentID::new(AGENT_ID_1).unwrap();
    let agent_id_2 = AgentID::new(AGENT_ID_2).unwrap();

    let instance_id_getter = InstanceIDWithIdentifiersGetter::new_k8s_instance_id_getter(
        k8s_store,
        Identifiers::default(),
    );

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
    // This test covers the happy path of YAMLConfigRepositoryConfigMap on K8s.

    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());

    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime(), test_ns.clone()).unwrap());
    let k8s_store = Arc::new(K8sStore::new(k8s_client));
    let agent_id_1 = AgentID::new(AGENT_ID_1).unwrap();
    let agent_id_2 = AgentID::new(AGENT_ID_2).unwrap();
    let mut value_repository = YAMLConfigRepositoryConfigMap::new(k8s_store.clone());
    let default_values = YAMLConfig::default();
    let capabilities = default_capabilities();
    // without values the default is expected
    let res = load_remote_fallback_local(&value_repository, &agent_id_1, &capabilities);
    assert_eq!(res.unwrap(), default_values);

    // with local values we expect some data
    block_on(create_local_config_map(
        test.client.clone(),
        test_ns.as_str(),
        "k8s_value_repository_config_map",
        format!("local-data-{}", AGENT_ID_1).as_str(),
    ));
    let local_values = YAMLConfig::try_from("test: 1".to_string()).unwrap();
    let res = load_remote_fallback_local(&value_repository, &agent_id_1, &capabilities);

    assert_eq!(res.unwrap(), local_values);

    // with remote data we expect we get local without remote
    let remote_values = YAMLConfig::try_from("test: 3".to_string()).unwrap();
    value_repository
        .store_remote(&agent_id_1, &remote_values)
        .unwrap();
    let res = load_remote_fallback_local(&value_repository, &agent_id_1, &capabilities);
    assert_eq!(res.unwrap(), local_values);

    // Once we have remote enabled we get remote data
    value_repository = value_repository.with_remote();
    let res = load_remote_fallback_local(&value_repository, &agent_id_1, &capabilities);
    assert_eq!(res.unwrap(), remote_values);

    // After deleting remote we expect to get still local data
    value_repository.delete_remote(&agent_id_1).unwrap();
    let res = load_remote_fallback_local(&value_repository, &agent_id_1, &capabilities);
    assert_eq!(res.unwrap(), local_values);

    // After saving data for a second agent should not affect the previous one
    // with remote data we expect to ignore local one
    let remote_values_agent_2 = YAMLConfig::try_from("test: 100".to_string()).unwrap();
    value_repository
        .store_remote(&agent_id_2, &remote_values_agent_2)
        .unwrap();
    let res = load_remote_fallback_local(&value_repository, &agent_id_1, &capabilities);
    let res_agent_2 = load_remote_fallback_local(&value_repository, &agent_id_2, &capabilities);
    assert_eq!(res.unwrap(), local_values);
    assert_eq!(res_agent_2.unwrap(), remote_values_agent_2);
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_sa_config_map() {
    // This test covers the happy path of SubAgentConfigStorerConfigMap on K8s.

    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());
    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime(), test_ns.clone()).unwrap());
    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));

    // This is the cached local config
    let agents_cfg_local = r#"
agents:
  infra-agent-a:
    agent_type: "com.newrelic.infrastructure:0.0.2"
  infra-agent-b:
    agent_type: "com.newrelic.infrastructure:0.0.2"
  infra-agent-c:
    agent_type: "com.newrelic.infrastructure:0.0.2"
  infra-agent-d:
    agent_type: "com.newrelic.infrastructure:0.0.2"
"#
    .to_string();

    block_on(create_config_map(
        test.client.clone(),
        test_ns.as_str(),
        K8sStore::build_cm_name(&AgentID::new_agent_control_id(), CM_NAME_LOCAL_DATA_PREFIX)
            .as_str(),
        agents_cfg_local,
    ));

    let vr = YAMLConfigRepositoryConfigMap::new(k8s_store.clone());
    let store_sa = AgentControlConfigStore::new(Arc::new(vr));
    assert_eq!(store_sa.load().unwrap().agents.len(), 4);

    // after removing an agent and storing it, we expect not to see it without remote enabled
    let agents_cfg = r#"
agents:
  infra-agent-a:
    agent_type: "com.newrelic.infrastructure:0.0.2"
  infra-agent-b:
    agent_type: "com.newrelic.infrastructure:0.0.2"
  not-infra-agent:
    agent_type: "io.opentelemetry.collector:0.1.0"
"#;
    assert!(store_sa
        .store(&from_str::<YAMLConfig>(agents_cfg).unwrap())
        .is_ok());
    assert_eq!(store_sa.load().unwrap().agents.len(), 4);

    // After enabling remote we can load the "remote" config
    let vr = YAMLConfigRepositoryConfigMap::new(k8s_store).with_remote();
    let store_sa = AgentControlConfigStore::new(Arc::new(vr));
    assert_eq!(store_sa.load().unwrap().agents.len(), 3);

    // After deleting the remote config the local one is loaded
    assert!(store_sa.delete().is_ok());
    assert_eq!(store_sa.load().unwrap().agents.len(), 4);
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
    let instance_id_getter = InstanceIDWithIdentifiersGetter::new_k8s_instance_id_getter(
        k8s_store.clone(),
        Identifiers::default(),
    );

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
