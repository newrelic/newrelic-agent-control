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
use newrelic_agent_control::on_host::file_store::{FileStore, build_config_name};
use newrelic_agent_control::values::ConfigRepo;
use newrelic_agent_control::values::config_repository::ConfigRepository;

use crate::common::config::AgentControlCommonConfigBuilder;

pub struct OnHostAgentControlConfigBuilder {
    common: AgentControlCommonConfigBuilder,

    oci_registry: Option<String>,
    oci_basic_auth: Option<(String, String)>,
    proxy: Option<String>,
    self_update: Option<SelfUpdateConfig>,
    agent_types: Option<AgentTypes>,
}

struct SelfUpdateConfig {
    signature_verification_enabled: bool,
    repository: String,
    public_key_url: String,
}

struct AgentTypes {
    signature_verification_enabled: bool,
    repository: String,
    public_key_url: String,
}

impl OnHostAgentControlConfigBuilder {
    pub fn new(opamp_endpoint: impl Into<String>, jwks_endpoint: impl Into<String>) -> Self {
        Self {
            common: AgentControlCommonConfigBuilder::default()
                .with_fleet(opamp_endpoint, jwks_endpoint),
            oci_registry: None,
            oci_basic_auth: None,
            proxy: None,
            self_update: None,
            agent_types: None,
        }
    }

    pub fn with_agents(mut self, agents: impl Into<String>) -> Self {
        self.common.agents = Some(agents.into());
        self
    }

    pub fn with_oci_registry(mut self, oci_registry: impl Into<String>) -> Self {
        self.oci_registry = Some(oci_registry.into());
        self
    }

    pub fn with_oci_basic_auth(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.oci_basic_auth = Some((username.into(), password.into()));
        self
    }

    pub fn with_status_server(mut self, port: u16) -> Self {
        self.common.status_server_port = Some(port);
        self
    }

    // This is used in `proxy.rs`, which isn't automated.
    // Therefore, we need to allow dead code here.
    pub fn with_self_update(
        mut self,
        signature_verification_enabled: bool,
        repository: impl Into<String>,
        public_key_url: impl Into<String>,
    ) -> Self {
        self.self_update = Some(SelfUpdateConfig {
            signature_verification_enabled,
            repository: repository.into(),
            public_key_url: public_key_url.into(),
        });
        self
    }

    pub fn with_agent_types(
        mut self,
        signature_verification_enabled: bool,
        repository: impl Into<String>,
        public_key_url: impl Into<String>,
    ) -> Self {
        self.agent_types = Some(AgentTypes {
            signature_verification_enabled,
            repository: repository.into(),
            public_key_url: public_key_url.into(),
        });
        self
    }

    #[allow(dead_code)]
    pub fn with_proxy(mut self, proxy: impl Into<String>) -> Self {
        self.proxy = Some(proxy.into());
        self
    }

    pub fn write(self, local_dir: PathBuf) {
        let fleet_control_config = self.common.build_fleet_control_yaml();
        let agents_config = self.common.build_agents_yaml();
        let status_server_config = self.common.build_server_yaml();

        let proxy_config = self
            .proxy
            .map(|p| format!("proxy: {p}"))
            .unwrap_or_default();

        let oci_config = self
            .oci_registry
            .map(|r| {
                let auth = self
                    .oci_basic_auth
                    .map(|(username, password)| {
                        format!(
                            "  auth:\n    basic:\n      username: {username}\n      password: {password}"
                        )
                    })
                    .unwrap_or_default();

                format!("oci:\n  registry: \"{}\"\n{}", r, auth)
            })
            .unwrap_or_default();

        let self_update_config = self
            .self_update
            .map(|su| {
                format!(
                    r#"self_update:
  enabled: true
  signature_verification_enabled: {}
  package:
    download:
      oci:
        repository: {}
        public_key_url: {}"#,
                    su.signature_verification_enabled, su.repository, su.public_key_url,
                )
            })
            .unwrap_or_default();

        let agent_types_config = self
            .agent_types
            .map(|at| {
                format!(
                    r#"agent_types:
  default_remote:
    repository: {}
    signature_verification_enabled: {}
    public_key_url: {}"#,
                    at.repository, at.signature_verification_enabled, at.public_key_url,
                )
            })
            .unwrap_or_default();

        let agent_control_config = format!(
            r#"
host_id: integration-test
{fleet_control_config}
{agents_config}
{proxy_config}
{oci_config}
{status_server_config}
{self_update_config}
{agent_types_config}
"#,
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
