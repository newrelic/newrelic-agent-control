use std::fs::create_dir_all;
use std::path::PathBuf;
use std::sync::Arc;

use fs::file::LocalFile;
use fs::file::writer::FileWriter;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_ID, FOLDER_NAME_FLEET_DATA, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
    STORE_KEY_OPAMP_DATA_CONFIG, default_capabilities,
};
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::on_host::OCI_TEST_REGISTRY_URL;
use newrelic_agent_control::on_host::file_store::{FileStore, build_config_name};
use newrelic_agent_control::values::ConfigRepo;
use newrelic_agent_control::values::config_repository::ConfigRepository;

pub struct AgentControlConfigBuilder {
    opamp_endpoint: String,
    jwks_endpoint: String,
    agents: String,
    oci_registry: String,
    status_server_port: Option<u16>,
    proxy: Option<String>,
}

impl AgentControlConfigBuilder {
    pub fn new(
        opamp_endpoint: impl Into<String>,
        jwks_endpoint: impl Into<String>,
        agents: impl Into<String>,
    ) -> Self {
        Self {
            opamp_endpoint: opamp_endpoint.into(),
            jwks_endpoint: jwks_endpoint.into(),
            agents: agents.into(),
            oci_registry: OCI_TEST_REGISTRY_URL.to_string(),
            status_server_port: None,
            proxy: None,
        }
    }

    pub fn with_oci_registry(mut self, registry: impl Into<String>) -> Self {
        self.oci_registry = registry.into();
        self
    }

    pub fn with_status_server(mut self, port: u16) -> Self {
        self.status_server_port = Some(port);
        self
    }

    // This is used in `proxy.rs`, which isn't automatized.
    // Therefore, we need to allow dead code here.
    #[allow(dead_code)]
    pub fn with_proxy(mut self, proxy: impl Into<String>) -> Self {
        self.proxy = Some(proxy.into());
        self
    }

    pub fn write(self, local_dir: PathBuf) {
        let proxy_config = self
            .proxy
            .map(|p| format!("proxy: {p}"))
            .unwrap_or_default();

        let status_server_config = self
            .status_server_port
            .map(|port| {
                format!(
                    r#"server:
  enabled: true
  port: {port}"#
                )
            })
            .unwrap_or_default();

        let agent_control_config = format!(
            r#"
host_id: integration-test
fleet_control:
  endpoint: {opamp_endpoint}
  poll_interval: 5s
  signature_validation:
    public_key_server_url: {jwks_endpoint}
oci:
  registry: "{oci_registry}"
agents: {agents}
{proxy_config}
{status_server_config}
"#,
            opamp_endpoint = self.opamp_endpoint,
            jwks_endpoint = self.jwks_endpoint,
            oci_registry = self.oci_registry,
            agents = self.agents,
        );

        create_file(
            agent_control_config,
            local_dir
                .join(FOLDER_NAME_LOCAL_DATA)
                .join(AGENT_CONTROL_ID)
                .join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)),
        );
    }
}

pub fn create_file(content: impl Into<String>, path: PathBuf) {
    create_dir_all(path.parent().unwrap()).unwrap();

    LocalFile
        .write(&path, content.into())
        .expect("failed to create file");
}

/// Creates local values config for the agent_id provided on the base_dir
/// with the given content.
pub fn create_local_config(
    agent_id: impl Into<String>,
    config: impl Into<String>,
    base_dir: PathBuf,
) {
    let agent_values_dir_path = base_dir.join(FOLDER_NAME_LOCAL_DATA).join(agent_id.into());
    create_dir_all(agent_values_dir_path.clone()).expect("failed to create values directory");

    let values_file_path =
        agent_values_dir_path.join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG));

    create_file(config, values_file_path.clone());
}

/// Creates remote values config for the agent_id provided on the base_dir
/// with the given content.
pub fn create_remote_config(agent_id: String, config: String, base_dir: PathBuf) {
    let agent_values_dir_path = base_dir.join(FOLDER_NAME_FLEET_DATA).join(agent_id);
    create_dir_all(agent_values_dir_path.clone()).expect("failed to create values directory");

    let values_file_path =
        agent_values_dir_path.join(build_config_name(STORE_KEY_OPAMP_DATA_CONFIG));

    create_file(config, values_file_path.clone());
}

pub fn load_remote_config_content(agent_id: &AgentID, base_paths: BasePaths) -> Option<String> {
    let file_store = Arc::new(FileStore::new_local_fs(
        base_paths.local_dir.clone(),
        base_paths.remote_dir.clone(),
    ));
    let yaml_config_repo = ConfigRepo::new(file_store).with_remote();

    yaml_config_repo
        .load_remote(agent_id, &default_capabilities())
        .unwrap()
        .map(|rc| serde_saphyr::to_string(&rc.get_yaml_config()).unwrap())
}
