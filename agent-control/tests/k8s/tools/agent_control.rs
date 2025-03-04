use super::k8s_api::check_config_map_exist;
use crate::common::{
    agent_control::{start_agent_control_with_custom_config, StartedAgentControl},
    retry::retry,
    runtime::block_on,
};
use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::{
    api::{Api, DeleteParams, PostParams},
    Client,
};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_CONFIG_FILENAME, DYNAMIC_AGENT_TYPE_FILENAME,
};
use newrelic_agent_control::{
    agent_control::run::BasePaths,
    k8s::store::{K8sStore, CM_NAME_LOCAL_DATA_PREFIX, STORE_KEY_LOCAL_DATA_CONFIG},
};
use std::io::Read;
use std::path::Path;
use std::time::Duration;
use std::{collections::BTreeMap, path::PathBuf};
use std::{fs::File, io::Write};

pub const TEST_CLUSTER_NAME: &str = "minikube";
pub const CUSTOM_AGENT_TYPE_PATH: &str = "tests/k8s/data/custom_agent_type.yml";
pub const CUSTOM_AGENT_TYPE_SECRET_PATH: &str = "tests/k8s/data/custom_agent_type_secret.yml";
pub const FOO_CR_AGENT_TYPE_PATH: &str = "tests/k8s/data/foo_cr_agent_type.yml";
pub const BAR_CR_AGENT_TYPE_PATH: &str = "tests/k8s/data/bar_cr_agent_type.yml";

/// Starts the agent-control through [start_agent_control] after setting up the corresponding configuration file
/// and config map according to the provided `folder_name` and the provided `file_names`.
#[allow(clippy::too_many_arguments)]
pub fn start_agent_control_with_testdata_config(
    folder_name: &str,
    dynamic_agent_type_path: &str,
    client: Client,
    ns: &str,
    remote_config_sign_cert: Option<PathBuf>,
    opamp_endpoint: Option<&str>,
    subagent_file_names: Vec<&str>,
    local_dir: &Path,
) -> StartedAgentControl {
    std::fs::copy(
        dynamic_agent_type_path,
        local_dir.join(DYNAMIC_AGENT_TYPE_FILENAME),
    )
    .unwrap();

    create_local_agent_control_config(
        client.clone(),
        ns,
        opamp_endpoint,
        remote_config_sign_cert,
        folder_name,
        local_dir,
    );
    for file_name in subagent_file_names {
        block_on(create_local_config_map(
            client.clone(),
            ns,
            folder_name,
            file_name,
        ))
    }
    start_agent_control_with_custom_config(BasePaths {
        local_dir: local_dir.to_path_buf(),
        remote_dir: local_dir.join("remote").to_path_buf(),
        log_dir: local_dir.join("log").to_path_buf(),
    })
}

/// Create a config map containing the configuration defined in the `{folder_name}/{name}` under the provided key.
/// If the file contains `<ns>`, the configuration is templated with the provided `ns` value.
pub async fn create_local_config_map(client: Client, ns: &str, folder_name: &str, name: &str) {
    let mut content = String::new();
    File::open(format!("tests/k8s/data/{}/{}.yaml", folder_name, name))
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();

    create_config_map(client, ns, name, content.replace("<ns>", ns)).await;
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
pub fn create_local_agent_control_config(
    client: Client,
    test_ns: &str,
    opamp_endpoint: Option<&str>,
    remote_config_sign_cert: Option<PathBuf>,
    folder_name: &str,
    tmp_dir: &Path,
) {
    let mut content = String::new();
    File::open(format!(
        "tests/k8s/data/{}/local-data-agent-control.template",
        folder_name
    ))
    .unwrap()
    .read_to_string(&mut content)
    .unwrap();

    let mut content = content
        .replace("<ns>", test_ns)
        .replace("<cluster-name>", TEST_CLUSTER_NAME);
    if let Some(endpoint) = opamp_endpoint {
        content = content.replace("<opamp-endpoint>", endpoint);
    }
    if let Some(cert_path) = remote_config_sign_cert {
        content = content.replace("<cert-path>", cert_path.to_str().unwrap());
    }
    block_on(create_config_map(
        client,
        test_ns,
        K8sStore::build_cm_name(&AgentID::new_agent_control_id(), CM_NAME_LOCAL_DATA_PREFIX)
            .as_str(),
        content.clone(),
    ));

    File::create(tmp_dir.join(AGENT_CONTROL_CONFIG_FILENAME))
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
