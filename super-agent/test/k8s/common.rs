use bollard::{
    container::{Config, LogsOptions},
    exec::{CreateExecOptions, StartExecResults},
    service::{HostConfig, PortBinding},
    Docker,
};
use futures::StreamExt;
use k8s_openapi::{
    api::core::v1::Namespace,
    apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition,
};
use kube::api::TypeMeta;
use kube::{
    api::{Api, DeleteParams, Patch, PatchParams, PostParams},
    Client, CustomResource, CustomResourceExt,
};
use newrelic_super_agent::config::{
    error::SuperAgentConfigError,
    super_agent_configs::{AgentTypeError, SuperAgentConfig},
};
use newrelic_super_agent::{config::super_agent_configs::AgentID, k8s::labels::Labels};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, env, fs::File, io::Write, time::Duration};
use tempfile::{tempdir, TempDir};
use tokio::sync::OnceCell;
use tokio::time::timeout;

const KUBECONFIG_PATH: &str = "test/k8s/.kubeconfig-dev";
const K3S_BOOTSTRAP_TIMEOUT: u64 = 60;
const K3S_IMAGE_ENV: &str = "K3S_IMAGE";
const K3S_CLUSTER_PORT: &str = "6443/tcp";

pub struct K8sEnv {
    pub client: Client,
    generated_namespaces: Vec<String>,
}

impl K8sEnv {
    pub async fn new() -> Self {
        // Forces the client to use the dev kubeconfig file.
        env::set_var("KUBECONFIG", KUBECONFIG_PATH);

        let client = Client::try_default().await.expect("fail to create client");
        create_foo_crd(client.to_owned()).await;

        K8sEnv {
            client,
            generated_namespaces: Vec::new(),
        }
    }

    pub async fn test_namespace(&mut self) -> String {
        let mut test_namespace = Namespace::default();
        test_namespace.metadata.generate_name = Some("super-agent-test-".to_string());

        let namespaces: Api<Namespace> = Api::all(self.client.clone());

        let created_namespace = namespaces
            .create(&PostParams::default(), &test_namespace)
            .await
            .expect("fail to create test namespace");

        let ns = created_namespace
            .metadata
            .name
            .ok_or("fail getting the ns")
            .unwrap();

        self.generated_namespaces.push(ns.clone());

        ns
    }

    async fn clean_up(&self) {
        let namespaces: Api<Namespace> = Api::all(self.client.clone());

        for ns in self.generated_namespaces.iter() {
            namespaces
                .delete(ns.as_str(), &DeleteParams::default())
                .await
                .expect("fail to remove namespace");
        }
    }
}

impl Drop for K8sEnv {
    fn drop(&mut self) {
        // clean up test environment even if the test panics.
        // async drop doesn't exist so this needs to be run sync code.
        newrelic_super_agent::runtime::runtime().block_on(self.clean_up());
    }
}

/// Structure that represents a spawned k8s cluster running in a container.
/// The container is removed when this object goes out of the scope.
/// In a similar way that running:
/// docker run \
///   --privileged \
///   --name k3s-server-1 \
///   --hostname k3s-server-1 \
///   -p 6443:6443 \
///   -d rancher/k3s:v1.28.2-k3s1 \
///   server
///
/// docker cp k3s-server-1:/etc/rancher/k3s/k3s.yaml ./kubeconfig-dev
pub struct K8sCluster {
    pub client: Option<Client>,
    docker: Docker,
    k3s_container_id: String,
    kubeconfig_dir: TempDir,
}

impl K8sCluster {
    pub async fn new() -> Self {
        let docker = Docker::connect_with_socket_defaults().expect("fail to connect to Docker");

        let container_id = docker
            .create_container::<String, String>(
                None,
                container_config(String::from(K3S_CLUSTER_PORT)),
            )
            .await
            .expect("fail to create container")
            .id;

        docker
            .start_container::<String>(&container_id, None)
            .await
            .expect("fail to start container");

        // Create the object just after the container is created in case clean up needed.
        let mut k8s_cluster = K8sCluster {
            docker,
            client: None,
            k3s_container_id: container_id.to_owned(),
            // TempDir is removed when dir gets dropped.
            kubeconfig_dir: tempdir().unwrap(),
        };

        timeout(
            Duration::from_secs(K3S_BOOTSTRAP_TIMEOUT),
            // based on https://github.com/testcontainers/testcontainers-go/blob/v0.26.0/modules/k3s/k3s.go#L62
            k8s_cluster.wait_log("Node controller sync successful"),
        )
        .await
        .expect("timeout waiting for k3s to be ready");

        println!("#### K3S Ready ####");

        k8s_cluster.set_kubeconfig().await;

        let client = Client::try_default()
            .await
            .expect("fail to create the client");

        k8s_cluster.client = Some(client.to_owned());

        create_foo_crd(client).await;

        k8s_cluster
    }

    async fn wait_log(&self, log: &str) {
        let mut logs = self.docker.logs(
            self.k3s_container_id.as_str(),
            Some(LogsOptions::<String> {
                follow: true,
                stdout: true,
                stderr: true,
                ..Default::default()
            }),
        );

        while let Ok(log_output) = logs.next().await.unwrap() {
            if log_output.to_string().contains(log) {
                break;
            }
        }
    }

    async fn set_kubeconfig(&self) {
        let exec = self
            .docker
            .create_exec(
                self.k3s_container_id.as_str(),
                CreateExecOptions {
                    attach_stdout: Some(true),
                    cmd: Some(vec!["cat", "/etc/rancher/k3s/k3s.yaml"]),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let mut kubeconfig = String::new();

        if let StartExecResults::Attached { mut output, .. } = self
            .docker
            .start_exec(&exec.id, None)
            .await
            .expect("fail to exec")
        {
            while let Some(Ok(msg)) = output.next().await {
                kubeconfig = msg.to_string();
            }
        }

        let file_path = self.kubeconfig_dir.path().join("kubeconfig-dev");

        writeln!(File::create(&file_path).unwrap(), "{}", kubeconfig).unwrap();

        env::set_var("KUBECONFIG", &file_path);

        println!("KUBECONFIG={}", &file_path.as_path().display());
    }

    async fn clean_up(&self) {
        self.docker
            .stop_container(self.k3s_container_id.as_str(), None)
            .await
            .expect("fail to stop the container");
        self.docker
            .remove_container(self.k3s_container_id.as_str(), None)
            .await
            .expect("fail to remove the container");
    }
}

impl Drop for K8sCluster {
    fn drop(&mut self) {
        // clean up test environment even if the test panics.
        // async drop doesn't exist so this needs to be run sync code.
        newrelic_super_agent::runtime::runtime().block_on(self.clean_up());
    }
}

fn container_config(cluster_port: String) -> Config<String> {
    let mut port_bindings = HashMap::new();
    port_bindings.insert(
        cluster_port.to_owned(),
        Some(vec![PortBinding {
            host_port: Some(cluster_port.to_owned()),
            host_ip: Some(String::from("0.0.0.0")),
        }]),
    );

    let mut exposed_ports = HashMap::new();
    exposed_ports.insert(cluster_port.to_owned(), HashMap::new());

    // k3s image is pulled outside to avoid handle of credentials inside here.
    // In order to have single source of truth for that image we pass it by envar from the makefile
    let image = env::var(K3S_IMAGE_ENV).expect("missing k3s image env var");

    Config::<String> {
        image: Some(image),
        exposed_ports: Some(exposed_ports),
        host_config: Some(HostConfig {
            privileged: Some(true),
            port_bindings: Some(port_bindings),
            publish_all_ports: Some(true),
            ..Default::default()
        }),
        cmd: Some(vec![
            String::from("server"),
            String::from("--disable-helm-controller"),
            String::from("--disable=traefik,metrics-server"),
        ]),
        ..Default::default()
    }
}

#[derive(Default, CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(group = "newrelic.com", version = "v1", kind = "Foo", namespaced)]
pub struct FooSpec {
    pub data: String,
}

pub fn foo_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "newrelic.com/v1".to_string(),
        kind: "Foo".to_string(),
    }
}

static ONCE: OnceCell<()> = OnceCell::const_new();

/// Create the Foo CRD for testing purposes.The CRD is not cleaned on test termination (for simplicity) so all tests
/// can assume this CRD exists.
pub async fn create_foo_crd(client: Client) {
    ONCE.get_or_try_init(|| async { perform_crd_patch(client).await })
        .await
        .expect("Error creating the Foo CRD");

    // Wait for the CRD to be fully deployed: https://github.com/kubernetes/kubectl/issues/1117
    tokio::time::sleep(Duration::from_secs(1)).await;
}

async fn perform_crd_patch(client: Client) -> Result<(), kube::Error> {
    let crds: Api<CustomResourceDefinition> = Api::all(client);
    crds.patch(
        "foos.newrelic.com",
        &PatchParams::apply("foo"),
        &Patch::Apply(Foo::crd()),
    )
    .await?;
    Ok(())
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

mock! {
    pub SuperAgentConfigLoader {}

    impl newrelic_super_agent::config::store::SuperAgentConfigLoader for SuperAgentConfigLoader {
        fn load(&self) -> Result<SuperAgentConfig, SuperAgentConfigError>;
    }
}
