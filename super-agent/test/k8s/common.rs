use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_test_env::{
    env::K8sEnv,
    foo_crd::{Foo, FooSpec},
    runtime::block_on,
};
use kube::api::DynamicObject;
use kube::core::GroupVersion;
use kube::{
    api::{Api, DeleteParams, PostParams},
    Client,
};
use newrelic_super_agent::{
    k8s::{labels::Labels, store::STORE_KEY_LOCAL_DATA_CONFIG},
    super_agent::{
        config::{AgentID, AgentTypeError, SuperAgentConfig, SuperAgentConfigError},
        config_storer::storer::{SuperAgentConfigLoader, SuperAgentDynamicConfigLoader},
    },
};
use std::error::Error;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::{collections::BTreeMap, path::PathBuf};
use std::{fs::File, io::Write, time::Duration};

const KUBECONFIG_PATH: &str = "test/k8s/.kubeconfig-dev";

pub async fn k8s_env() -> K8sEnv {
    K8sEnv::new(KUBECONFIG_PATH).await
}

/// Creates a Foo CR for testing purposes.
/// ### Panics
/// It panics if there is an error creating the CR.
pub async fn create_test_cr(client: Client, namespace: &str, name: &str) -> Foo {
    let api: Api<Foo> = Api::namespaced(client, namespace);
    let mut foo_cr = Foo::new(
        name,
        FooSpec {
            data: String::from("test"),
        },
    );

    let agent_id = match AgentID::new(name) {
        Err(AgentTypeError::InvalidAgentIDUsesReservedOne(_)) => AgentID::new_super_agent_id(),
        Ok(id) => id,
        _ => panic!(),
    };

    foo_cr.metadata.labels = Some(Labels::new(&agent_id).get());

    foo_cr = api.create(&PostParams::default(), &foo_cr).await.unwrap();

    // Sleeping to let watchers have the time to be updated
    tokio::time::sleep(Duration::from_secs(1)).await;

    foo_cr
}

use mockall::mock;
use newrelic_super_agent::super_agent::config::SuperAgentDynamicConfig;

mock! {
    pub SuperAgentConfigLoader {}

    impl SuperAgentConfigLoader for SuperAgentConfigLoader {
        fn load(&self) -> Result<SuperAgentConfig, SuperAgentConfigError>;
    }
}

mock! {
    pub SuperAgentDynamicConfigLoaderMock{}

    impl SuperAgentDynamicConfigLoader for SuperAgentDynamicConfigLoaderMock {
        fn load(&self) -> Result<SuperAgentDynamicConfig, SuperAgentConfigError>;
    }
}

pub fn start_super_agent_with_testdata_config(
    folder_name: &str,
    client: Client,
    ns: &str,
    opamp_endpoint: &str,
    subagent_file_names: Vec<&str>,
) -> std::process::Child {
    let config_local = create_local_sa_config(ns, opamp_endpoint, folder_name);
    for file_name in subagent_file_names {
        block_on(create_mock_config_maps(
            client.clone(),
            ns,
            folder_name,
            file_name,
            STORE_KEY_LOCAL_DATA_CONFIG,
        ))
    }
    start_super_agent(&config_local)
}

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

pub async fn create_mock_config_maps(
    client: Client,
    test_ns: &str,
    folder_name: &str,
    name: &str,
    key: &str,
) {
    let cm_client: Api<ConfigMap> = Api::<ConfigMap>::namespaced(client, test_ns);
    let mut content = String::new();
    File::open(format!("test/k8s/data/{}/{}.yaml", folder_name, name))
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();

    let mut data = BTreeMap::new();
    data.insert(key.to_string(), content.replace("<ns>", test_ns));

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

/// create_local_sa_config templates the namespace and the opamp endpoint, and then it saves the new file whose path
/// is returned.
pub fn create_local_sa_config(
    test_ns: &str,
    opamp_endpoint: &str,
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
    let content = content
        .replace("<ns>", test_ns)
        .replace("<opamp-endpoint>", opamp_endpoint);
    File::create(file_path.as_str())
        .unwrap()
        .write_all(content.as_bytes())
        .unwrap();
    PathBuf::from(file_path)
}

pub fn retry<F>(max_attempts: usize, interval: Duration, f: F)
where
    F: Fn() -> Result<(), Box<dyn std::error::Error>>,
{
    let mut last_err = Ok(());
    for _ in 0..max_attempts {
        if let Err(err) = f() {
            last_err = Err(err)
        } else {
            return;
        }
        std::thread::sleep(interval);
    }
    last_err.unwrap_or_else(|err| panic!("retry failed after {max_attempts} attempts: {err}"))
}

/// check_deployments_exist checks for the existence of specified deployments within a namespace.
pub async fn check_deployments_exist(
    k8s_client: Client,
    names: &[&str],
    namespace: &str,
) -> Result<(), Box<dyn Error>> {
    let api: Api<Deployment> = Api::namespaced(k8s_client.clone(), namespace);

    for &name in names {
        let _ = api.get(name).await.map_err(|err| {
            std::convert::Into::<Box<dyn Error>>::into(format!(
                "Deployment {name} not found: {err}"
            ))
        })?;
    }
    Ok(())
}

pub async fn check_helmrelease_spec_values(
    k8s_client: Client,
    namespace: &str,
    name: &str,
    expected_spec_values: &str,
) -> Result<(), Box<dyn Error>> {
    let expected_as_json: serde_json::Value = serde_yaml::from_str(expected_spec_values).unwrap();
    let gvk = &GroupVersion::from_str("helm.toolkit.fluxcd.io/v2beta2")
        .unwrap()
        .with_kind("HelmRelease");
    let (api_resource, _) = kube::discovery::pinned_kind(&k8s_client, gvk).await?;
    let api: Api<DynamicObject> =
        Api::namespaced_with(k8s_client.clone(), namespace, &api_resource);

    let obj = api.get(name).await?;
    let found_values = &obj.data["spec"]["values"];
    if expected_as_json != *found_values {
        return Err(format!(
            "helm release spec values don't match with expected. Expected: {:?}, Found: {:?}",
            expected_as_json, *found_values,
        )
        .into());
    }
    Ok(())
}
