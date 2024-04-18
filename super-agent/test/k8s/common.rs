use bollard::{
    container::{Config, LogsOptions},
    exec::{CreateExecOptions, StartExecResults},
    service::{HostConfig, PortBinding},
    Docker,
};
use futures::{Future, StreamExt};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::{
    api::core::v1::Namespace,
    apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition,
};
use kube::api::{DynamicObject, TypeMeta};
use kube::core::GroupVersion;
use kube::{
    api::{Api, DeleteParams, Patch, PatchParams, PostParams},
    Client, CustomResource, CustomResourceExt,
};
use newrelic_super_agent::{
    event::channel::EventPublisher,
    event::OpAMPEvent,
    k8s::labels::Labels,
    opamp::client_builder::{OpAMPClientBuilder, OpAMPClientBuilderError},
    super_agent::{
        config::{AgentID, AgentTypeError, SuperAgentConfig, SuperAgentConfigError},
        config_storer::storer::SuperAgentConfigLoader,
        config_storer::storer::SuperAgentDynamicConfigLoader,
    },
};
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::operation::settings::StartSettings;
use opamp_client::Client as OpAMPClient;
use opamp_client::{
    opamp::proto::{AgentDescription, ComponentHealth, RemoteConfigStatus},
    ClientResult, NotStartedClient, NotStartedClientResult, StartedClient, StartedClientResult,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::Arc;
use std::{collections::BTreeMap, path::PathBuf};
use std::{collections::HashMap, env, fs::File, io::Write, sync::OnceLock, time::Duration};
use tempfile::NamedTempFile;
use tempfile::{tempdir, TempDir};
use tokio::{runtime::Runtime, sync::OnceCell, time::timeout};

const KUBECONFIG_PATH: &str = "test/k8s/.kubeconfig-dev";
const K3S_BOOTSTRAP_TIMEOUT: u64 = 60;
const K3S_IMAGE_ENV: &str = "K3S_IMAGE";
const K3S_CLUSTER_PORT: &str = "6443/tcp";

/// Returns a static reference to the tokio runtime. The runtime is built the first time this function
/// is called.
pub fn tokio_runtime() -> Arc<Runtime> {
    static RUNTIME_ONCE: OnceLock<Arc<Runtime>> = OnceLock::new();
    RUNTIME_ONCE
        .get_or_init(|| {
            Arc::new(
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .unwrap(),
            )
        })
        .clone()
}

/// A wrapper to shorten the usage of the runtime's block_on. It is useful because most synchronous
/// tests need to perform some calls to async functions.
pub fn block_on<F: Future>(future: F) -> F::Output {
    tokio_runtime().block_on(future)
}

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
}

impl Drop for K8sEnv {
    fn drop(&mut self) {
        // clean up test environment even if the test panics.
        // 'async drop' doesn't exist so `block_on` is needed to run it synchronously.
        //
        // Since K8sEnv variables can be dropped from either sync or async code, we need an additional runtime to make
        // it work.
        //
        // `futures::executor::block_on` is needed because we cannot execute `runtime.block_on` from a tokio
        // context (such as `#[tokio::test]`) as it would fail with:
        // ```
        // 'Cannot start a runtime from within a runtime. This happens because a function (like `block_on`) attempted to block the current thread while the thread is being used to drive asynchronous tasks.'
        // ````
        // It is important to notice that the usage of `futures::executor::block_on` could lead to a dead-lock if there
        // are not available threads in the tokio runtime, so we need to use the multi-threading version of the macro:
        // `#[tokio::test(flavor = "multi_thread")]`
        //
        // `runtime.spawn(<future-block>).await` is needed because we cannot execute `futures::executor::block_on` when there is
        // no tokio runtime (synchronous tests), since it would fail with:
        // ```
        // 'there is no reactor running, must be called from the context of a Tokio 1.x runtime
        // ```
        futures::executor::block_on(async move {
            let ns_api: Api<Namespace> = Api::all(self.client.clone());
            let generated_namespaces = self.generated_namespaces.clone();
            tokio_runtime()
                .spawn(async move {
                    for ns in generated_namespaces.into_iter() {
                        ns_api
                            .delete(ns.as_str(), &DeleteParams::default())
                            .await
                            .expect("fail to remove namespace");
                    }
                })
                .await
                .unwrap();
        })
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
}

impl Drop for K8sCluster {
    fn drop(&mut self) {
        // clean up test environment even if the test panics.
        // 'async drop' doesn't exist so `block_on` is needed to run it synchronously.
        // It is implemented following the same strategy than [K8sEnv::drop]
        futures::executor::block_on(async move {
            let docker = self.docker.clone();
            let container_id = self.k3s_container_id.clone();
            tokio_runtime()
                .spawn(async move {
                    docker
                        .stop_container(container_id.as_str(), None)
                        .await
                        .expect("fail to stop the container");
                    docker
                        .remove_container(container_id.as_str(), None)
                        .await
                        .expect("fail to remove the container");
                })
                .await
                .unwrap();
        })
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

pub async fn get_dynamic_api_foo(client: kube::Client, test_ns: String) -> Api<DynamicObject> {
    let gvk = &GroupVersion::from_str(foo_type_meta().api_version.as_str())
        .unwrap()
        .with_kind(foo_type_meta().kind.as_str());
    let (ar, _) = kube::discovery::pinned_kind(&client.to_owned(), gvk)
        .await
        .unwrap();
    let api: Api<DynamicObject> = Api::namespaced_with(client.to_owned(), test_ns.as_str(), &ar);
    api
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
use newrelic_super_agent::super_agent::config::SuperAgentDynamicConfig;
use tokio::time::sleep;

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

// check_deployments_exist checks for the existence of specified deployments within a namespace,
// retrying a given number of times with pauses between attempts. Panics with an assert if any
// deployment is not found after the specified retries.
pub async fn check_deployments_exist(
    k8s_client: Client,
    names: &[&str],
    namespace: &str,
    max_retries: usize,
    retry_interval: Duration,
) {
    let api: Api<Deployment> = Api::namespaced(k8s_client.clone(), namespace);

    for &name in names {
        let mut found = false;
        for _ in 0..max_retries {
            if let Ok(_) = api.get(name).await {
                found = true;
                break;
            }
            sleep(retry_interval).await;
        }
        assert!(found, "Deployment {} not found after retries", name);
    }
}

// OpAMP mocks //
/////////////////
mock! {
    pub NotStartedOpAMPClientMock {}
    impl NotStartedClient for NotStartedOpAMPClientMock
     {
        type StartedClient<C: Callbacks + Send + Sync + 'static> = MockStartedOpAMPClientMock<C>;
        fn start<C: Callbacks + Send + Sync + 'static>(self, callbacks: C, start_settings: StartSettings ) -> NotStartedClientResult<<Self as NotStartedClient>::StartedClient<C>>;
    }
}

mock! {
    pub StartedOpAMPClientMock<C> where C: Callbacks {}

    impl<C> StartedClient<C> for StartedOpAMPClientMock<C>
        where
        C: Callbacks + Send + Sync + 'static {

        fn stop(self) -> StartedClientResult<()>;
    }

    impl<C> OpAMPClient for StartedOpAMPClientMock<C>
    where
    C: Callbacks + Send + Sync + 'static {

         fn set_agent_description(
            &self,
            description: AgentDescription,
        ) -> ClientResult<()>;

         fn set_health(&self, health: ComponentHealth) -> ClientResult<()>;

         fn update_effective_config(&self) -> ClientResult<()>;

         fn set_remote_config_status(&self, status: RemoteConfigStatus) -> ClientResult<()>;
    }
}

impl<C> MockStartedOpAMPClientMock<C>
where
    C: Callbacks + Send + Sync + 'static,
{
    pub fn should_set_health(&mut self, times: usize) {
        self.expect_set_health().times(times).returning(|_| Ok(()));
    }

    pub fn should_set_any_remote_config_status(&mut self, times: usize) {
        self.expect_set_remote_config_status()
            .times(times)
            .returning(|_| Ok(()));
    }
}

mock! {
    pub OpAMPClientBuilderMock<C> where C: Callbacks + Send + Sync + 'static{}

    impl<C> OpAMPClientBuilder<C> for OpAMPClientBuilderMock<C> where C: Callbacks + Send + Sync + 'static{
        type Client = MockStartedOpAMPClientMock<C>;
        fn build_and_start(&self, opamp_publisher: EventPublisher<OpAMPEvent>, agent_id: AgentID, start_settings: StartSettings) -> Result<<Self as OpAMPClientBuilder<C>>::Client, OpAMPClientBuilderError>;
    }
}

pub async fn check_helmrelease_spec_values(
    k8s_client: Client,
    namespace: &str,
    name: &str,
    expected_spec_values: &str,
    max_retries: usize,
    retry_interval: Duration,
) {
    let expected_as_json: serde_json::Value = serde_yaml::from_str(expected_spec_values).unwrap();
    let gvk = &GroupVersion::from_str("helm.toolkit.fluxcd.io/v2beta2")
        .unwrap()
        .with_kind("HelmRelease");
    let (api_resource, _) = kube::discovery::pinned_kind(&k8s_client, gvk)
        .await
        .unwrap();
    let api: Api<DynamicObject> =
        Api::namespaced_with(k8s_client.clone(), namespace, &api_resource);

    let mut issue = None;
    for _ in 0..max_retries {
        let obj_result = api.get(name).await;
        match obj_result {
            Ok(obj) => {
                let found_values = &obj.data["spec"]["values"];
                if expected_as_json == *found_values {
                    issue = None;
                    break;
                } else {
                    issue = Some(format!("helm release spec values don't match with expected. Expected: {:?}, Found: {:?}", expected_as_json, *found_values));
                }
            }
            Err(obj_err) => {
                issue = Some(obj_err.to_string());
            }
        }
        sleep(retry_interval).await;
    }
    if let Some(issue) = issue {
        panic!("The helmrelease does not match after retries: {issue}");
    }
}
