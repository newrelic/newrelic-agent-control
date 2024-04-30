use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::{
    api::{Api, DeleteParams, PostParams},
    Client,
};
use newrelic_super_agent::k8s::store::STORE_KEY_LOCAL_DATA_CONFIG;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::{collections::BTreeMap, path::PathBuf};
use std::{fs::File, io::Write};

use super::runtime::block_on;

/// Starts the super-agent through [start_super_agent] after setting up the corresponding configuration file
/// and config map according to the provided `folder_name` and the provided `file_names`.
pub fn start_super_agent_with_testdata_config(
    folder_name: &str,
    client: Client,
    ns: &str,
    opamp_endpoint: Option<&str>,
    subagent_file_names: Vec<&str>,
) -> std::process::Child {
    let config_local = create_local_super_agent_config(ns, opamp_endpoint, folder_name);
    for file_name in subagent_file_names {
        block_on(create_local_config_map(
            client.clone(),
            ns,
            folder_name,
            file_name,
        ))
    }
    start_super_agent(&config_local)
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
    let cm_client: Api<ConfigMap> = Api::<ConfigMap>::namespaced(client, ns);
    let mut content = String::new();
    File::open(format!("test/k8s/data/{}/{}.yaml", folder_name, name))
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();

    let mut data = BTreeMap::new();
    data.insert(
        STORE_KEY_LOCAL_DATA_CONFIG.to_string(),
        content.replace("<ns>", ns),
    );

    let cm = ConfigMap {
        binary_data: None,
        data: Some(data),
        immutable: None,
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            ..Default::default()
        },
    };

    // Making sure to clean up the cluster first
    _ = cm_client.delete(name, &DeleteParams::default()).await;
    cm_client.create(&PostParams::default(), &cm).await.unwrap();
}

/// Templates the namespace and the endpoint in `local-data-super-agent.template` file in the corresponding `folder_name`
/// and returns the resulting file path.
pub fn create_local_super_agent_config(
    test_ns: &str,
    opamp_endpoint: Option<&str>,
    folder_name: &str,
) -> std::path::PathBuf {
    let mut content = String::new();
    File::open(format!(
        "test/k8s/data/{}/local-data-super-agent.template",
        folder_name
    ))
    .unwrap()
    .read_to_string(&mut content)
    .unwrap();

    let file_path = format!("test/k8s/data/{}/local-sa.k8s_tmp", folder_name);
    let mut content = content.replace("<ns>", test_ns);
    if let Some(endpoint) = opamp_endpoint {
        content = content.replace("<opamp-endpoint>", endpoint);
    }
    File::create(file_path.as_str())
        .unwrap()
        .write_all(content.as_bytes())
        .unwrap();
    PathBuf::from(file_path)
}
