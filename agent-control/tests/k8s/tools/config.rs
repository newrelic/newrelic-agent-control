use std::io::Write;
use std::path::Path;
use std::{fs, fs::File};

use kube::Client;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_ID, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
};
use newrelic_agent_control::k8s::configmap_store::ConfigMapStore;
use newrelic_agent_control::on_host::file_store::build_config_name;

use crate::common::config::AgentControlCommonConfigBuilder;
use crate::common::runtime::block_on;
use crate::k8s::tools::agent_control::{
    K8S_KEY_SECRET, K8S_PRIVATE_KEY_SECRET, TEST_CLUSTER_NAME, create_config_map,
};

pub struct K8sAgentControlConfigBuilder {
    ac_namespace: String,
    common: AgentControlCommonConfigBuilder,
    namespace_agents: Option<String>,
    cd_enabled: Option<bool>,
    cd_remote_update: Option<bool>,
    ac_remote_update: Option<bool>,
    cd_release_name: Option<String>,
    current_chart_version: Option<String>,
    secret_private_key_name: Option<String>,
    cr_type_meta: Option<String>,
    secrets_providers: Option<String>,
}

impl K8sAgentControlConfigBuilder {
    pub fn new(ac_namespace: impl Into<String>) -> Self {
        Self {
            ac_namespace: ac_namespace.into(),
            common: AgentControlCommonConfigBuilder::default(),
            namespace_agents: None,
            cd_enabled: None,
            cd_remote_update: None,
            ac_remote_update: None,
            cd_release_name: None,
            current_chart_version: None,
            secret_private_key_name: None,
            cr_type_meta: None,
            secrets_providers: None,
        }
    }

    pub fn with_fleet(
        mut self,
        opamp_endpoint: impl Into<String>,
        jwks_endpoint: impl Into<String>,
    ) -> Self {
        self.common = self.common.with_fleet(opamp_endpoint, jwks_endpoint);
        self
    }

    pub fn with_agents(mut self, agents: impl Into<String>) -> Self {
        self.common.agents = Some(agents.into());
        self
    }

    pub fn with_status_server(mut self, port: u16) -> Self {
        self.common.status_server_port = Some(port);
        self
    }

    pub fn with_signature_validation_disabled(mut self) -> Self {
        self.common.signature_validation_disabled = true;
        self
    }

    pub fn with_namespace_agents(mut self, namespace_agents: impl Into<String>) -> Self {
        self.namespace_agents = Some(namespace_agents.into());
        self
    }

    // Clippy complains about dead code in windows tests, but this is used in Linux tests.
    #[allow(dead_code)]
    pub fn with_cd_enabled(mut self, enabled: bool) -> Self {
        self.cd_enabled = Some(enabled);
        self
    }

    pub fn with_cd_remote_update(mut self, enabled: bool) -> Self {
        self.cd_remote_update = Some(enabled);
        self
    }

    pub fn with_ac_remote_update(mut self, enabled: bool) -> Self {
        self.ac_remote_update = Some(enabled);
        self
    }

    pub fn with_cd_release_name(mut self, name: impl Into<String>) -> Self {
        self.cd_release_name = Some(name.into());
        self
    }

    pub fn with_current_chart_version(mut self, version: impl Into<String>) -> Self {
        self.current_chart_version = Some(version.into());
        self
    }

    // Clippy complains about dead code in windows tests, but this is used in Linux tests.
    #[allow(dead_code)]
    pub fn with_secret_private_key_name(mut self, name: impl Into<String>) -> Self {
        self.secret_private_key_name = Some(name.into());
        self
    }

    pub fn with_cr_type_meta(mut self, cr_type_meta: impl Into<String>) -> Self {
        self.cr_type_meta = Some(cr_type_meta.into());
        self
    }

    pub fn with_secrets_providers(mut self, secrets_providers: impl Into<String>) -> Self {
        self.secrets_providers = Some(secrets_providers.into());
        self
    }

    pub fn write(self, client: Client, local_dir: &Path) {
        let ac_ns = self.ac_namespace.clone();
        let content = self.build_yaml();

        block_on(create_config_map(
            client,
            &ac_ns,
            ConfigMapStore::build_cm_name(&AgentID::AgentControl, FOLDER_NAME_LOCAL_DATA).as_str(),
            content.clone(),
        ));

        let local = local_dir
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(AGENT_CONTROL_ID);
        fs::create_dir_all(&local).unwrap();
        File::create(local.join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)))
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();
    }

    fn build_yaml(self) -> String {
        let ac_ns = &self.ac_namespace;
        let namespace_agents = self.namespace_agents.as_deref().unwrap_or(ac_ns);

        let mut k8s_block = format!(
            r#"k8s:
  namespace: {ac_ns}
  namespace_agents: {namespace_agents}
  cluster_name: {TEST_CLUSTER_NAME}
  auth_secret:
    secret_name: {K8S_PRIVATE_KEY_SECRET}
    secret_key_name: {K8S_KEY_SECRET}"#
        );

        if let Some(name) = &self.secret_private_key_name {
            k8s_block.push_str(&format!("\n  secret_private_key_name: {name}"));
        }
        if let Some(enabled) = self.cd_enabled {
            k8s_block.push_str(&format!("\n  cd_enabled: {enabled}"));
        }
        if let Some(enabled) = self.cd_remote_update {
            k8s_block.push_str(&format!("\n  cd_remote_update: {enabled}"));
        }
        if let Some(enabled) = self.ac_remote_update {
            k8s_block.push_str(&format!("\n  ac_remote_update: {enabled}"));
        }
        if let Some(name) = &self.cd_release_name {
            k8s_block.push_str(&format!("\n  cd_release_name: {name}"));
        }
        if let Some(version) = &self.current_chart_version {
            k8s_block.push_str(&format!("\n  current_chart_version: {version}"));
        }
        if let Some(cr_type_meta) = &self.cr_type_meta {
            k8s_block.push_str(&format!("\n  cr_type_meta:\n{cr_type_meta}"));
        }

        let secrets_providers_block = self
            .secrets_providers
            .map(|sp| format!("secrets_providers:\n{sp}"))
            .unwrap_or_default();

        let parts: Vec<String> = [
            self.common.build_fleet_control_yaml(),
            self.common.build_agents_yaml(),
            self.common.build_server_yaml(),
            k8s_block,
            secrets_providers_block,
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();

        parts.join("\n")
    }
}
