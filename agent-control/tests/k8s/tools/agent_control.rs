use super::k8s_api::{check_config_map_exist, create_values_secret};
use crate::common::{
    agent_control::{StartedAgentControl, start_agent_control_with_custom_config},
    retry::retry,
    runtime::block_on,
};
use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::{
    Client,
    api::{Api, DeleteParams, PostParams},
};
use newrelic_agent_control::agent_control::defaults::STORE_KEY_LOCAL_DATA_CONFIG;
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::environment::Environment;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

pub const TEST_CLUSTER_NAME: &str = "minikube";

pub const CUSTOM_AGENT_TYPE_PATH: &str = "tests/k8s/data/custom_agent_type.yml";
pub const CUSTOM_AGENT_TYPE_SPLIT_NS_PATH: &str = "tests/k8s/data/custom_agent_type_split_ns.yml";
pub const CUSTOM_AGENT_TYPE_SECRETS_PATH: &str = "tests/k8s/data/custom_agent_type_secrets.yml";
pub const CUSTOM_AGENT_TYPE_DIRECT_CHECKS_PATH: &str =
    "tests/k8s/data/custom_agent_type_direct_checks.yml";
pub const FOO_CR_AGENT_TYPE_PATH: &str = "tests/k8s/data/foo_cr_agent_type.yml";
pub const BAR_CR_AGENT_TYPE_PATH: &str = "tests/k8s/data/bar_cr_agent_type.yml";

pub const DYNAMIC_AGENT_TYPE_FILENAME: &str = "dynamic-agent-types/type.yaml";

pub const K8S_PRIVATE_KEY_SECRET: &str = "agent-control-auth";
pub const K8S_KEY_SECRET: &str = "private_key";

pub const DUMMY_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDCt
-----END PRIVATE KEY-----"#;

/// Starts agent-control after the config has already been written via [K8sAgentControlConfigBuilder].
/// Copies the dynamic agent type file, creates the auth secret, and starts the process.
pub fn start_agent_control(
    dynamic_agent_type_path: &str,
    client: Client,
    ac_ns: &str,
    local_dir: &Path,
) -> StartedAgentControl {
    let agent_type_file_path = local_dir.join(DYNAMIC_AGENT_TYPE_FILENAME);
    std::fs::create_dir_all(agent_type_file_path.parent().unwrap()).unwrap();
    std::fs::copy(dynamic_agent_type_path, agent_type_file_path).unwrap();

    create_values_secret(
        client,
        ac_ns,
        K8S_PRIVATE_KEY_SECRET,
        K8S_KEY_SECRET,
        DUMMY_PRIVATE_KEY.to_string(),
    );

    start_agent_control_with_custom_config(
        BasePaths {
            local_dir: local_dir.to_path_buf(),
            remote_dir: local_dir.join("remote").to_path_buf(),
            log_dir: local_dir.join("log").to_path_buf(),
        },
        Environment::K8s,
    )
}

pub async fn create_config_map(client: Client, ns: &str, name: &str, content: String) {
    let mut data = BTreeMap::new();
    data.insert(STORE_KEY_LOCAL_DATA_CONFIG.to_string(), content);

    let cm = ConfigMap {
        data: Some(data),
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            ..Default::default()
        },
        ..Default::default()
    };

    // Making sure to clean up the cluster first
    let cm_client: Api<ConfigMap> = Api::<ConfigMap>::namespaced(client, ns);
    _ = cm_client.delete(name, &DeleteParams::default()).await;
    cm_client.create(&PostParams::default(), &cm).await.unwrap();
}

/// This function checks that the cm containing the instance id of the agentControl has been created.
/// If it is present we assume that the AgentControl was started and was able to connect to the cluster.
pub fn wait_until_agent_control_with_opamp_is_started(k8s_client: Client, namespace: &str) {
    // check that the expected cm exist, meaning that the SA started
    retry(30, Duration::from_secs(1), || {
        block_on(check_config_map_exist(
            k8s_client.clone(),
            "fleet-data-agent-control",
            namespace,
        ))
    });
}
