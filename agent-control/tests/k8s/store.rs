use crate::common::runtime::{block_on, tokio_runtime};
use crate::k8s::tools::agent_control::{create_config_map, create_local_config_map};
use crate::k8s::tools::k8s_env::K8sEnv;
use k8s_openapi::api::core::v1::ConfigMap;
use kube::Api;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::config_repository::repository::AgentControlDynamicConfigRepository;
use newrelic_agent_control::agent_control::config_repository::store::AgentControlConfigStore;
use newrelic_agent_control::agent_control::defaults::{
    FOLDER_NAME_FLEET_DATA, STORE_KEY_INSTANCE_ID, STORE_KEY_OPAMP_DATA_CONFIG,
};
use newrelic_agent_control::agent_control::defaults::{
    FOLDER_NAME_LOCAL_DATA, default_capabilities,
};
use newrelic_agent_control::k8s::client::SyncK8sClient;
use newrelic_agent_control::k8s::labels::Labels;
use newrelic_agent_control::k8s::store::K8sStore;
use newrelic_agent_control::opamp::data_store::StoreKey;
use newrelic_agent_control::opamp::instance_id::getter::{
    InstanceIDGetter, InstanceIDWithIdentifiersGetter,
};
use newrelic_agent_control::opamp::instance_id::k8s::getter::Identifiers;
use newrelic_agent_control::opamp::instance_id::storer::GenericStorer;
use newrelic_agent_control::opamp::remote_config::hash::{ConfigState, Hash};
use newrelic_agent_control::values::GenericConfigRepository;
use newrelic_agent_control::values::config::RemoteConfig;
use newrelic_agent_control::values::config_repository::ConfigRepository;
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

    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime()).unwrap());
    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone(), test_ns.clone()));

    let agent_id_1 = AgentID::try_from(AGENT_ID_1).unwrap();
    let agent_id_2 = AgentID::try_from(AGENT_ID_2).unwrap();

    let instance_id_storer = GenericStorer::from(k8s_store.clone());
    let instance_id_getter = InstanceIDWithIdentifiersGetter::new(
        instance_id_storer,
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
fn k8s_hash_in_config_map() {
    // This test covers the happy path of Getting the hash from a RemoteConfig from the ConfigMap on K8s.
    // It checks that with same AgentID the Hash is the same and if different the Hash is different

    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());

    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime()).unwrap());
    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone(), test_ns.clone()));
    let agent_id_1 = AgentID::try_from(AGENT_ID_1).unwrap();
    let agent_id_2 = AgentID::try_from(AGENT_ID_2).unwrap();

    let config_repository = GenericConfigRepository::new(k8s_store);

    assert!(
        config_repository
            .get_remote_config(&agent_id_1)
            .unwrap()
            .is_none()
    );

    let hash_1 = Hash::from("hash-test");
    let remote_config_1 = RemoteConfig {
        config: YAMLConfig::default(),
        hash: hash_1.clone(),
        state: ConfigState::Applying,
    };
    config_repository
        .store_remote(&agent_id_1, &remote_config_1)
        .unwrap();
    let loaded_hash_1 = config_repository
        .get_remote_config(&agent_id_1)
        .unwrap()
        .unwrap()
        .hash;
    assert_eq!(hash_1, loaded_hash_1);

    let hash2 = Hash::from("hash-test2");
    let remote_config_2 = RemoteConfig {
        config: YAMLConfig::default(),
        hash: hash2.clone(),
        state: ConfigState::Applying,
    };
    config_repository
        .store_remote(&agent_id_2, &remote_config_2)
        .unwrap();
    let loaded_hash_2 = config_repository
        .get_remote_config(&agent_id_2)
        .unwrap()
        .unwrap()
        .hash;
    assert_eq!(hash2, loaded_hash_2);

    let cm_client: Api<ConfigMap> =
        Api::<ConfigMap>::namespaced(test.client.clone(), test_ns.as_str());
    assert_agent_cm(&cm_client, &agent_id_1, STORE_KEY_OPAMP_DATA_CONFIG);
    assert_agent_cm(&cm_client, &agent_id_2, STORE_KEY_OPAMP_DATA_CONFIG);
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_value_repository_config_map() {
    // This test covers the happy path of ConfigRepositoryConfigMap on K8s.

    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());

    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime()).unwrap());
    let k8s_store = Arc::new(K8sStore::new(k8s_client, test_ns.clone()));
    let agent_id_1 = AgentID::try_from(AGENT_ID_1).unwrap();
    let agent_id_2 = AgentID::try_from(AGENT_ID_2).unwrap();
    let mut value_repository = GenericConfigRepository::new(k8s_store.clone());
    let capabilities = default_capabilities();
    // without values the none is expected
    let res = value_repository.load_remote_fallback_local(&agent_id_1, &capabilities);
    assert!(res.unwrap().is_none());

    // with local values we expect some data
    block_on(create_local_config_map(
        test.client.clone(),
        test_ns.as_str(),
        test_ns.as_str(),
        "k8s_value_repository_config_map",
        format!("local-data-{AGENT_ID_1}").as_str(),
    ));
    let local_values = YAMLConfig::try_from("test: 1".to_string()).unwrap();
    let res = value_repository
        .load_remote_fallback_local(&agent_id_1, &capabilities)
        .expect("unexpected error loading config")
        .expect("expected some configuration, got None");

    assert_eq!(res.get_yaml_config().clone(), local_values);

    // with remote data we expect we get local without remote
    let remote_values = RemoteConfig {
        config: YAMLConfig::try_from("test: 3".to_string()).unwrap(),
        hash: Hash::from("hash-test1"),
        state: ConfigState::Applied,
    };
    value_repository
        .store_remote(&agent_id_1, &remote_values)
        .unwrap();
    let res = value_repository.load_remote_fallback_local(&agent_id_1, &capabilities);
    assert_eq!(
        res.unwrap().unwrap().get_yaml_config().clone(),
        local_values
    );

    // Once we have remote enabled we get remote data
    value_repository = value_repository.with_remote();
    let res = value_repository
        .load_remote_fallback_local(&agent_id_1, &capabilities)
        .expect("unexpected error loading config")
        .expect("expected some configuration, got None");
    assert_eq!(res.get_yaml_config().clone(), remote_values.config);
    assert_eq!(res.get_hash(), Some(&remote_values.hash));

    // After deleting remote we expect to get still local data
    value_repository.delete_remote(&agent_id_1).unwrap();
    let res = value_repository
        .load_remote_fallback_local(&agent_id_1, &capabilities)
        .expect("unexpected error loading config")
        .expect("expected some configuration, got None");
    assert_eq!(res.get_yaml_config().clone(), local_values);

    // After saving data for a second agent should not affect the previous one
    // with remote data we expect to ignore local one
    let remote_values_agent_2 = RemoteConfig {
        config: YAMLConfig::try_from("test: 100".to_string()).unwrap(),
        hash: Hash::from("hash-test2"),
        state: ConfigState::Applied,
    };

    value_repository
        .store_remote(&agent_id_2, &remote_values_agent_2)
        .unwrap();
    let res = value_repository
        .load_remote_fallback_local(&agent_id_1, &capabilities)
        .expect("unexpected error loading config")
        .expect("expected some configuration, got None");
    let res_agent_2 = value_repository
        .load_remote_fallback_local(&agent_id_2, &capabilities)
        .expect("unexpected error loading config")
        .expect("expected some configuration, got None");
    assert_eq!(res.get_yaml_config().clone(), local_values);
    assert_eq!(
        res_agent_2.get_yaml_config().clone(),
        remote_values_agent_2.config
    );
    assert_eq!(res_agent_2.get_hash(), Some(&remote_values_agent_2.hash));
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_sa_config_map() {
    // This test covers the happy path of SubAgentConfigStorerConfigMap on K8s.

    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());
    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime()).unwrap());
    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone(), test_ns.clone()));

    // This is the cached local config
    let agents_cfg_local = r#"
agents:
  infra-agent-a:
    agent_type: "newrelic/com.newrelic.infrastructure:0.0.2"
  infra-agent-b:
    agent_type: "newrelic/com.newrelic.infrastructure:0.0.2"
  infra-agent-c:
    agent_type: "newrelic/com.newrelic.infrastructure:0.0.2"
  infra-agent-d:
    agent_type: "newrelic/com.newrelic.infrastructure:0.0.2"
"#
    .to_string();

    block_on(create_config_map(
        test.client.clone(),
        test_ns.as_str(),
        K8sStore::build_cm_name(&AgentID::AgentControl, FOLDER_NAME_LOCAL_DATA).as_str(),
        agents_cfg_local,
    ));

    let vr = GenericConfigRepository::new(k8s_store.clone());
    let store_sa = AgentControlConfigStore::new(Arc::new(vr));
    assert_eq!(store_sa.load().unwrap().agents.len(), 4);

    // after removing an agent and storing it, we expect not to see it without remote enabled
    let agents_cfg = r#"
agents:
  infra-agent-a:
    agent_type: "newrelic/com.newrelic.infrastructure:0.0.2"
  infra-agent-b:
    agent_type: "newrelic/com.newrelic.infrastructure:0.0.2"
  not-infra-agent:
    agent_type: "newrelic/com.newrelic.opentelemetry.collector:0.1.0"
"#;
    let remote_values_agent = RemoteConfig {
        config: from_str::<YAMLConfig>(agents_cfg).unwrap(),
        hash: Hash::from("hash-test3"),
        state: ConfigState::Applied,
    };
    assert!(store_sa.store(&remote_values_agent).is_ok());
    assert_eq!(store_sa.load().unwrap().agents.len(), 4);

    // After enabling remote we can load the "remote" config
    let vr = GenericConfigRepository::new(k8s_store).with_remote();
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

    let k8s_client = Arc::new(SyncK8sClient::try_new(tokio_runtime()).unwrap());
    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone(), test_ns.clone()));
    let agent_id = AgentID::try_from(AGENT_ID_1).unwrap();

    // Persisters sharing the ConfigMap
    let config_repository = GenericConfigRepository::new(k8s_store.clone());
    let instance_id_storer = GenericStorer::from(k8s_store.clone());
    let instance_id_getter =
        InstanceIDWithIdentifiersGetter::new(instance_id_storer, Identifiers::default());

    let hash = Hash::from("hash-test");
    let remote_config = RemoteConfig {
        config: YAMLConfig::default(),
        hash: hash.clone(),
        state: ConfigState::Applying,
    };
    config_repository
        .store_remote(&agent_id, &remote_config)
        .unwrap();
    let instance_id_created = instance_id_getter.get(&agent_id).unwrap();

    // Assert from loaded entries
    assert_eq!(
        hash,
        config_repository
            .get_remote_config(&agent_id)
            .unwrap()
            .unwrap()
            .hash
    );
    assert_eq!(
        instance_id_created,
        instance_id_getter.get(&agent_id).unwrap()
    );

    let cm_client: Api<ConfigMap> =
        Api::<ConfigMap>::namespaced(test.client.clone(), test_ns.as_str());
    assert_agent_cm(&cm_client, &agent_id, STORE_KEY_OPAMP_DATA_CONFIG);
    assert_agent_cm(&cm_client, &agent_id, STORE_KEY_INSTANCE_ID);
}

fn assert_agent_cm(cm_client: &Api<ConfigMap>, agent_id: &AgentID, store_key: &StoreKey) {
    let cm_name = K8sStore::build_cm_name(agent_id, FOLDER_NAME_FLEET_DATA);
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
