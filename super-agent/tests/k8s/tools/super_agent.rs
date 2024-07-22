use crate::common::{retry::retry, runtime::block_on};
use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::{
    api::{Api, DeleteParams, PostParams},
    Client,
};
use newrelic_super_agent::k8s::store::{
    K8sStore, CM_NAME_LOCAL_DATA_PREFIX, STORE_KEY_LOCAL_DATA_CONFIG,
};
use newrelic_super_agent::super_agent::config::AgentID;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use std::{collections::BTreeMap, path::PathBuf};
use std::{fs::File, io::Write};

use super::k8s_api::check_config_map_exist;

pub const TEST_CLUSTER_NAME: &str = "minikube";

/// Starts the super-agent through [start_super_agent] after setting up the corresponding configuration file
/// and config map according to the provided `folder_name` and the provided `file_names`.
pub fn start_super_agent_with_testdata_config(
    folder_name: &str,
    client: Client,
    ns: &str,
    opamp_endpoint: Option<&str>,
    subagent_file_names: Vec<&str>,
) -> AutoDroppingChild {
    let config_local =
        create_local_super_agent_config(client.clone(), ns, opamp_endpoint, folder_name);
    for file_name in subagent_file_names {
        block_on(create_local_config_map(
            client.clone(),
            ns,
            folder_name,
            file_name,
        ))
    }
    AutoDroppingChild {
        child: start_super_agent(&config_local),
    }
}

pub struct AutoDroppingChild {
    pub child: std::process::Child,
}

impl Drop for AutoDroppingChild {
    fn drop(&mut self) {
        println!("Killing SuperAgent Process");
        self.child.kill().expect("Failed to kill child process");
    }
}

/// Starts the super-agent compiled with the k8s feature and the provided configuration file.
pub fn start_super_agent(file_path: &Path) -> std::process::Child {
    let mut command = Command::new("cargo");
    command
        .args([
            "run",
            "--bin",
            "newrelic-super-agent",
            "--features",
            "k8s",
            "--",
            "--config",
        ])
        .arg(file_path)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    command.spawn().expect("Failed to start super agent")
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

/// Templates the namespace and the endpoint in `local-data-super-agent.template` file in the corresponding `folder_name`
/// and returns the resulting file path.
pub fn create_local_super_agent_config(
    client: Client,
    test_ns: &str,
    opamp_endpoint: Option<&str>,
    folder_name: &str,
) -> std::path::PathBuf {
    let mut content = String::new();
    File::open(format!(
        "tests/k8s/data/{}/local-data-super-agent.template",
        folder_name
    ))
    .unwrap()
    .read_to_string(&mut content)
    .unwrap();

    let file_path = format!("tests/k8s/data/{}/local-sa.k8s_tmp", folder_name);
    let mut content = content
        .replace("<ns>", test_ns)
        .replace("<cluster-name>", TEST_CLUSTER_NAME);
    if let Some(endpoint) = opamp_endpoint {
        content = content.replace("<opamp-endpoint>", endpoint);
    }
    block_on(create_config_map(
        client,
        test_ns,
        K8sStore::build_cm_name(&AgentID::new_super_agent_id(), CM_NAME_LOCAL_DATA_PREFIX).as_str(),
        content.clone(),
    ));

    File::create(file_path.as_str())
        .unwrap()
        .write_all(content.as_bytes())
        .unwrap();
    PathBuf::from(file_path)
}

/// This function checks that the cm containing the instance id of the superAgent has been created.
/// If it is present we assume that the SuperAgent was started and was able to connect to the cluster.
pub fn wait_until_super_agent_with_opamp_is_started(k8s_client: Client, namespace: &str) {
    // check that the expected cm exist, meaning that the SA started
    retry(30, Duration::from_secs(5), || {
        block_on(check_config_map_exist(
            k8s_client.clone(),
            "opamp-data-super-agent",
            namespace,
        ))
    });
}
