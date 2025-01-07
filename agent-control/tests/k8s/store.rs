use crate::common::runtime::{block_on, tokio_runtime};
use crate::k8s::tools::agent_control::{create_config_map, create_local_config_map};
use crate::k8s::tools::k8s_env::K8sEnv;
use k8s_openapi::api::core::v1::ConfigMap;
use kube::Api;
use newrelic_agent_control::agent_control::config::AgentID;
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
    STORE_KEY_INSTANCE_ID, STORE_KEY_OPAMP_DATA_REMOTE_CONFIG_STATUS,
};
use newrelic_agent_control::opamp::instance_id::{
    getter::{InstanceIDGetter, InstanceIDWithIdentifiersGetter},
    Identifiers,
};
use newrelic_agent_control::opamp::remote_config::hash::Hash;
use newrelic_agent_control::opamp::remote_config::status::AgentRemoteConfigStatus;
use newrelic_agent_control::opamp::remote_config::status_manager::k8s::K8sConfigStatusManager;
use newrelic_agent_control::opamp::remote_config::status_manager::ConfigStatusManager;
use newrelic_agent_control::values::yaml_config::YAMLConfig;
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

    let config_manager = K8sConfigStatusManager::new(k8s_store).with_remote();

    assert_eq!(
        None,
        config_manager
            .retrieve_remote_status(&agent_id_1, &default_capabilities())
            .unwrap()
    );

    let hash_1 = Hash::new("hash-test".to_string());
    let mut remote_config_status = AgentRemoteConfigStatus {
        status_hash: hash_1.clone(),
        remote_config: None,
    };
    config_manager
        .store_remote_status(&agent_id_1, &remote_config_status)
        .unwrap();
    let loaded_hash_1 = config_manager
        .retrieve_remote_status(&agent_id_1, &default_capabilities())
        .unwrap()
        .unwrap()
        .status_hash;
    assert_eq!(hash_1, loaded_hash_1);

    let hash2 = Hash::new("hash-test2".to_string());
    remote_config_status.status_hash = hash2.clone();
    config_manager
        .store_remote_status(&agent_id_2, &remote_config_status)
        .unwrap();
    let loaded_hash_2 = config_manager
        .retrieve_remote_status(&agent_id_2, &default_capabilities())
        .unwrap()
        .unwrap()
        .status_hash;
    assert_eq!(hash2, loaded_hash_2);

    let cm_client: Api<ConfigMap> =
        Api::<ConfigMap>::namespaced(test.client.clone(), test_ns.as_str());
    assert_agent_cm(
        &cm_client,
        &agent_id_1,
        STORE_KEY_OPAMP_DATA_REMOTE_CONFIG_STATUS,
    );
    assert_agent_cm(
        &cm_client,
        &agent_id_2,
        STORE_KEY_OPAMP_DATA_REMOTE_CONFIG_STATUS,
    );
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
    let mut config_manager = K8sConfigStatusManager::new(k8s_store.clone());
    let default_values = YAMLConfig::default();
    let capabilities = default_capabilities();
    // without values the default is expected
    let res = config_manager.load_remote_fallback_local(&agent_id_1, &capabilities);
    assert_eq!(res.unwrap(), default_values);

    // with local values we expect some data
    block_on(create_local_config_map(
        test.client.clone(),
        test_ns.as_str(),
        "k8s_value_repository_config_map",
        format!("local-data-{}", AGENT_ID_1).as_str(),
    ));
    let local_values = YAMLConfig::try_from("test: 1".to_string()).unwrap();
    let res = config_manager.load_remote_fallback_local(&agent_id_1, &capabilities);

    assert_eq!(res.unwrap(), local_values);

    // with remote data we expect we get local without remote
    let hash = Hash::new("hash-test".to_string());
    let remote_values = YAMLConfig::try_from("test: 3".to_string()).unwrap();
    let remote_config_status = AgentRemoteConfigStatus {
        status_hash: hash,
        remote_config: Some(remote_values.clone()),
    };
    config_manager
        .store_remote_status(&agent_id_1, &remote_config_status)
        .unwrap();
    let res = config_manager.load_remote_fallback_local(&agent_id_1, &capabilities);
    assert_eq!(res.unwrap(), local_values);

    // Once we have remote enabled we get remote data
    config_manager = config_manager.with_remote();
    let res = config_manager.load_remote_fallback_local(&agent_id_1, &capabilities);
    assert_eq!(res.unwrap(), remote_values);

    // After deleting remote we expect to get still local data
    config_manager.delete_remote_status(&agent_id_1).unwrap();
    let res = config_manager.load_remote_fallback_local(&agent_id_1, &capabilities);
    assert_eq!(res.unwrap(), local_values);

    // After saving data for a second agent should not affect the previous one
    // with remote data we expect to ignore local one
    let hash = Hash::new("hash-test".to_string());
    let remote_values_agent_2 = YAMLConfig::try_from("test: 100".to_string()).unwrap();
    let remote_config_status_agent_2 = AgentRemoteConfigStatus {
        status_hash: hash,
        remote_config: Some(remote_values_agent_2.clone()),
    };
    config_manager
        .store_remote_status(&agent_id_2, &remote_config_status_agent_2)
        .unwrap();
    let res = config_manager.load_remote_fallback_local(&agent_id_1, &capabilities);
    let res_agent_2 = config_manager.load_remote_fallback_local(&agent_id_2, &capabilities);
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

    let vr = K8sConfigStatusManager::new(k8s_store.clone());
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
    agent_type: "io.opentelemetry.collector:0.2.0"
"#;
    let hash = Hash::new("hash-test".to_string());
    let remote_config_status = AgentRemoteConfigStatus {
        status_hash: hash,
        remote_config: Some(from_str::<YAMLConfig>(agents_cfg).unwrap()),
    };
    assert!(store_sa.store(&remote_config_status).is_ok());
    assert_eq!(store_sa.load().unwrap().agents.len(), 4);

    // After enabling remote we can load the "remote" config
    let vr = K8sConfigStatusManager::new(k8s_store).with_remote();
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
    let config_manager = K8sConfigStatusManager::new(k8s_store.clone()).with_remote();
    let instance_id_getter = InstanceIDWithIdentifiersGetter::new_k8s_instance_id_getter(
        k8s_store.clone(),
        Identifiers::default(),
    );

    // Add entries to from all persisters
    let hash = Hash::new("hash-test".to_string());
    let remote_config_status = AgentRemoteConfigStatus {
        status_hash: hash.clone(),
        remote_config: None,
    };
    config_manager
        .store_remote_status(&agent_id, &remote_config_status)
        .unwrap();
    let instance_id_created = instance_id_getter.get(&agent_id).unwrap();

    // Assert from loaded entries
    assert_eq!(
        hash,
        config_manager
            .retrieve_remote_status(&agent_id, &default_capabilities())
            .unwrap()
            .unwrap()
            .status_hash
    );
    assert_eq!(
        instance_id_created,
        instance_id_getter.get(&agent_id).unwrap()
    );

    let cm_client: Api<ConfigMap> =
        Api::<ConfigMap>::namespaced(test.client.clone(), test_ns.as_str());
    assert_agent_cm(
        &cm_client,
        &agent_id,
        STORE_KEY_OPAMP_DATA_REMOTE_CONFIG_STATUS,
    );
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
