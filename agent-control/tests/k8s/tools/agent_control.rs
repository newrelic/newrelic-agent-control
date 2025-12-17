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
use newrelic_agent_control::agent_control::{agent_id::AgentID, run::Environment};
use newrelic_agent_control::{
    agent_control::defaults::{
        AGENT_CONTROL_ID, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
    },
    on_host::file_store::build_config_name,
};
use newrelic_agent_control::{agent_control::run::BasePaths, k8s::configmap_store::ConfigMapStore};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::time::Duration;
use std::{fs::File, io::Write};

pub const TEST_CLUSTER_NAME: &str = "minikube";
pub const CUSTOM_AGENT_TYPE_PATH: &str = "tests/k8s/data/custom_agent_type.yml";
pub const CUSTOM_AGENT_TYPE_SPLIT_NS_PATH: &str = "tests/k8s/data/custom_agent_type_split_ns.yml";
pub const CUSTOM_AGENT_TYPE_SECRETS_PATH: &str = "tests/k8s/data/custom_agent_type_secrets.yml";
pub const FOO_CR_AGENT_TYPE_PATH: &str = "tests/k8s/data/foo_cr_agent_type.yml";
pub const BAR_CR_AGENT_TYPE_PATH: &str = "tests/k8s/data/bar_cr_agent_type.yml";

pub const DYNAMIC_AGENT_TYPE_FILENAME: &str = "dynamic-agent-types/type.yaml";

pub const K8S_PRIVATE_KEY_SECRET: &str = "agent-control-auth";
pub const K8S_KEY_SECRET: &str = "private_key";

pub const DUMMY_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDCt
-----END PRIVATE KEY-----"#;

/// Starts the agent-control through [start_agent_control] after setting up the corresponding configuration file
/// and config map according to the provided `folder_name` and the provided `file_names`.
#[allow(clippy::too_many_arguments)]
pub fn start_agent_control_with_testdata_config(
    folder_name: &str,
    dynamic_agent_type_path: &str,
    client: Client,
    ac_ns: &str,
    agents_ns: &str,
    opamp_endpoint: Option<&str>,
    jwks_endpoint: Option<&str>,
    subagent_file_names: Vec<&str>,
    local_dir: &Path,
) -> StartedAgentControl {
    let agent_type_file_path = local_dir.join(DYNAMIC_AGENT_TYPE_FILENAME);
    std::fs::create_dir_all(agent_type_file_path.parent().unwrap()).unwrap();

    std::fs::copy(dynamic_agent_type_path, agent_type_file_path).unwrap();

    // Take into account that if no `ac_release_name` config value is provided then
    // no health checker for AC will be created, so any test that relies on health for
    // assertions (like receiving a health message through a fake OpAMP server) will fail!
    // The same happens for `cd_release_name`.
    create_local_agent_control_config(
        client.clone(),
        ac_ns,
        agents_ns,
        opamp_endpoint,
        jwks_endpoint,
        folder_name,
        local_dir,
    );
    for file_name in subagent_file_names {
        block_on(create_local_config_map(
            client.clone(),
            ac_ns,
            agents_ns,
            folder_name,
            file_name,
        ))
    }

    create_values_secret(
        client.clone(),
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

/// Create a config map containing the configuration defined in the `{folder_name}/{name}` under the provided key.
/// If the file contains `<ns>`, the configuration is templated with the provided `ns` value.
pub async fn create_local_config_map(
    client: Client,
    ac_ns: &str,
    agents_ns: &str,
    folder_name: &str,
    name: &str,
) {
    let mut content = String::new();
    File::open(format!("tests/k8s/data/{folder_name}/{name}.yaml"))
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();

    create_config_map(
        client,
        ac_ns,
        name,
        content
            .replace("<ns>", ac_ns)
            .replace("<agents-ns>", agents_ns),
    )
    .await;
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

/// Templates the namespace and the endpoint in `local-data-agent-control.template` file in the corresponding `folder_name`
/// and returns the resulting file path.
#[allow(clippy::too_many_arguments)]
pub fn create_local_agent_control_config(
    client: Client,
    ac_ns: &str,
    agents_ns: &str,
    opamp_endpoint: Option<&str>,
    jwk_endpoint: Option<&str>,
    folder_name: &str,
    tmp_dir: &Path,
) {
    let mut content = String::new();
    File::open(format!(
        "tests/k8s/data/{folder_name}/local-data-agent-control.template"
    ))
    .unwrap()
    .read_to_string(&mut content)
    .unwrap();

    let mut content = content
        .replace("<ns>", ac_ns)
        .replace("<agents-ns>", agents_ns)
        .replace("<cluster-name>", TEST_CLUSTER_NAME);

    if let Some(endpoint) = opamp_endpoint {
        content = content.replace("<opamp-endpoint>", endpoint);
    }
    if let Some(endpoint) = jwk_endpoint {
        content = content.replace("<jwks-endpoint>", endpoint);
    }
    block_on(create_config_map(
        client,
        ac_ns,
        ConfigMapStore::build_cm_name(&AgentID::AgentControl, FOLDER_NAME_LOCAL_DATA).as_str(),
        content.clone(),
    ));

    let local = tmp_dir.join(FOLDER_NAME_LOCAL_DATA).join(AGENT_CONTROL_ID);
    std::fs::create_dir_all(&local).unwrap();

    File::create(local.join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)))
        .unwrap()
        .write_all(content.as_bytes())
        .unwrap();
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
