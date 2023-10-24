use bollard::{
    container::{Config, LogsOptions},
    errors::Error,
    exec::{CreateExecOptions, StartExecResults},
    service::{HostConfig, PortBinding},
    Docker,
};
use futures::StreamExt;
use k8s_openapi::api::core::v1::Namespace;
use kube::{
    api::{Api, DeleteParams, PostParams},
    Client,
};
use std::{collections::HashMap, env, fs::File, io::Write, time::Duration};
use tempfile::{tempdir, TempDir};
use tokio::time::timeout;

const KUBECONFIG_PATH: &str = "test/k8s/.kubeconfig-dev";
const K3S_BOOTSTRAP_TIMEOUT: u64 = 60;

pub struct K8sEnv {
    client: Client,
    generated_namespaces: Vec<String>,
}

impl K8sEnv {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Forces the client to use the dev kubeconfig file.
        env::set_var("KUBECONFIG", KUBECONFIG_PATH);

        Ok(K8sEnv {
            client: Client::try_default().await?,
            generated_namespaces: Vec::new(),
        })
    }

    pub async fn test_namespace(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        let mut test_namespace = Namespace::default();
        test_namespace.metadata.generate_name = Some("super-agent-test-".to_string());

        let namespaces: Api<Namespace> = Api::all(self.client.clone());

        let created_namespace = namespaces
            .create(&PostParams::default(), &test_namespace)
            .await?;

        let ns = created_namespace.metadata.name.unwrap();
        self.generated_namespaces.push(ns.clone());
        Ok(ns)
    }

    async fn clean_up(&self) {
        let namespaces: Api<Namespace> = Api::all(self.client.clone());

        for ns in self.generated_namespaces.iter() {
            let _ = namespaces
                .delete(ns.as_str(), &DeleteParams::default())
                .await;
        }
    }
}

impl Drop for K8sEnv {
    fn drop(&mut self) {
        // clean up test environment even if the test panics.
        // async drop doesn't exist so this needs to be run sync code.
        futures::executor::block_on(self.clean_up());
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
    docker: Docker,
    k3s_container_id: String,
    kubeconfig_dir: TempDir,
}

impl K8sCluster {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let docker = Docker::connect_with_socket_defaults()?;

        // TODO use unused random port to allow parallel clusters.
        let cluster_port = String::from("6443/tcp");

        let mut port_bindings = HashMap::new();
        port_bindings.insert(
            cluster_port.to_owned(),
            Some(vec![PortBinding {
                host_port: Some(cluster_port.to_owned()),
                host_ip: Some(String::from("0.0.0.0")),
            }]),
        );

        let mut exposed_ports = HashMap::new();
        exposed_ports.insert(cluster_port.as_str(), HashMap::new());

        // k3s image is pulled outside to avoid handle of credentials inside here.
        // In order to have single source of truth for that image we pass it by envar from the makefile
        let image = env::var("K3S_IMAGE").expect("K3S_IMAGE env var needs to be defined");

        let k3s_container_config = Config {
            image: Some(image.as_str()),
            exposed_ports: Some(exposed_ports),
            host_config: Some(HostConfig {
                privileged: Some(true),
                port_bindings: Some(port_bindings),
                publish_all_ports: Some(true),
                ..Default::default()
            }),
            cmd: Some(vec![
                "server",
                "--disable-helm-controller",
                "--disable=traefik,metrics-server",
            ]),
            ..Default::default()
        };

        let container_id = docker
            .create_container::<&str, &str>(None, k3s_container_config)
            .await?
            .id;

        docker
            .start_container::<String>(&container_id, None)
            .await?;

        // Create the object just after the container is created in case clean up needed.
        let k8s_cluster = K8sCluster {
            docker,
            k3s_container_id: container_id.to_owned(),
            // TempDir is removed when dir gets dropped.
            kubeconfig_dir: tempdir().unwrap(),
        };

        let _ = timeout(
            Duration::from_secs(K3S_BOOTSTRAP_TIMEOUT),
            // based on https://github.com/testcontainers/testcontainers-go/blob/v0.26.0/modules/k3s/k3s.go#L62
            k8s_cluster.wait_log("Node controller sync successful"),
        )
        .await?;

        println!("#### K3S Ready ####");

        k8s_cluster.set_kubeconfig().await?;

        Ok(k8s_cluster)
    }

    async fn wait_log(&self, log: &str) -> Result<(), Error> {
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
        Ok(())
    }

    async fn set_kubeconfig(&self) -> Result<(), Error> {
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
            .await?;

        let mut kubeconfig = String::new();

        if let StartExecResults::Attached { mut output, .. } =
            self.docker.start_exec(&exec.id, None).await?
        {
            while let Some(Ok(msg)) = output.next().await {
                kubeconfig = msg.to_string();
            }
        }

        let file_path = self.kubeconfig_dir.path().join("kubeconfig-dev");

        writeln!(File::create(&file_path)?, "{}", kubeconfig)?;

        env::set_var("KUBECONFIG", &file_path);

        println!("KUBECONFIG={}", &file_path.as_path().display());

        Ok(())
    }

    async fn clean_up(&self) {
        let _ = self
            .docker
            .stop_container(self.k3s_container_id.as_str(), None)
            .await;
        let _ = self
            .docker
            .remove_container(self.k3s_container_id.as_str(), None)
            .await;
    }
}

impl Drop for K8sCluster {
    fn drop(&mut self) {
        // clean up test environment even if the test panics.
        // async drop doesn't exist so this needs to be run sync code.
        futures::executor::block_on(self.clean_up());
    }
}
