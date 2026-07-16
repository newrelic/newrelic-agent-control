use crate::common::custom_agent_type::CommonCustomAgentTypeBuilder;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use newrelic_agent_control::agent_type::agent_type_id::AgentTypeID;
use std::fmt::Display;
use std::path::PathBuf;

pub const DYNAMIC_AGENT_TYPE_FILENAME: &str = "dynamic-agent-types/type.yaml";

/// Helper to build a Custom Agent type with defaults ready to use in integration tests
pub struct OnHostCustomAgentTypeBuilder {
    common: CommonCustomAgentTypeBuilder,
    executables: Option<serde_json::Value>,
    filesystem: Option<serde_json::Value>,
    shared_filesystem: Option<serde_json::Value>,
    packages: Option<serde_json::Value>,
    health: Option<serde_json::Value>,
}

impl Default for OnHostCustomAgentTypeBuilder {
    fn default() -> Self {
        Self {
            common: CommonCustomAgentTypeBuilder::new(Self::default_agent_type_id())
                .with_variables(
                    r#"
fake_variable:
  description: "fake variable to verify remote config"
  type: "string"
  required: false
  default: "default"
"#,
                ),
            executables: Some(Self::default_executables()),
            filesystem: None,
            shared_filesystem: None,
            packages: None,
            health: Some(
                serde_saphyr::from_str(
                    r#"
interval: 60s
initial_delay: 0s
timeout: 15s
checks:
  - kind: Process
"#,
                )
                .unwrap(),
            ),
        }
    }
}

impl Display for OnHostCustomAgentTypeBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let content = format!(
            r#"
        namespace: {}
        name: {}
        version: {}
        platform: host
        operating_system: {}
        protocol_version: "1.0"
        "#,
            self.common.agent_type_id.namespace(),
            self.common.agent_type_id.name(),
            self.common.agent_type_id.version(),
            AGENT_CONTROL_MODE_ON_HOST,
        );
        let mut content: serde_json::Map<String, serde_json::Value> =
            serde_saphyr::from_str(&content).unwrap();

        let variables = self
            .common
            .variables
            .clone()
            .unwrap_or_else(|| serde_json::Map::<String, serde_json::Value>::new().into());

        let mut deployment = serde_json::Map::<String, serde_json::Value>::new();
        if let Some(executables) = self.executables.as_ref() {
            deployment.insert("executables".into(), executables.clone());
        }
        if let Some(filesystem) = self.filesystem.as_ref() {
            deployment.insert("filesystem".into(), filesystem.clone());
        }
        if let Some(shared_filesystem) = self.shared_filesystem.as_ref() {
            deployment.insert("shared_filesystem".into(), shared_filesystem.clone());
        }
        if let Some(health) = self.health.as_ref() {
            deployment.insert("health".into(), health.clone());
        }
        if let Some(packages) = self.packages.as_ref() {
            deployment.insert("packages".into(), packages.clone());
        }

        content.insert("variables".into(), variables);
        content.insert("deployment".into(), deployment.into());
        let content = serde_json::Value::from(content);

        write!(f, "{}", serde_saphyr::to_string(&content).unwrap())
    }
}

impl OnHostCustomAgentTypeBuilder {
    fn default_agent_type_id() -> AgentTypeID {
        AgentTypeID::try_from("newrelic/com.newrelic.custom_agent:0.1.0").unwrap()
    }

    #[cfg(target_family = "unix")]
    fn default_executables() -> serde_json::Value {
        serde_saphyr::from_str(
            r#"
- id: "trap-term-sleep"
  path: "sh"
  args:
    - tests/on_host/data/sleep_60.sh
"#,
        )
        .unwrap()
    }

    #[cfg(target_family = "windows")]
    fn default_executables() -> serde_json::Value {
        serde_saphyr::from_str(
            r#"
- id: "trap-term-sleep"
  path: "powershell.exe"
  args:
    - -NoProfile
    - -ExecutionPolicy
    - Bypass
    - -File
    - tests\\on_host\\data\\sleep_60.ps1
"#,
        )
        .unwrap()
    }

    pub fn empty() -> Self {
        Self {
            common: CommonCustomAgentTypeBuilder::new(Self::default_agent_type_id()),
            executables: None,
            health: None,
            filesystem: None,
            shared_filesystem: None,
            packages: None,
        }
    }

    pub fn with_executables(mut self, executables: Option<&str>) -> Self {
        self.executables = executables.map(|e| serde_saphyr::from_str(e).unwrap());
        self
    }

    pub fn with_health(mut self, health: Option<&str>) -> Self {
        self.health = health.map(|h| serde_saphyr::from_str(h).unwrap());
        self
    }

    pub fn with_filesystem(mut self, filesystem: Option<&str>) -> Self {
        self.filesystem = filesystem.map(|f| serde_saphyr::from_str(f).unwrap());
        self
    }

    pub fn with_shared_filesystem(mut self, shared_filesystem: Option<&str>) -> Self {
        self.shared_filesystem = shared_filesystem.map(|f| serde_saphyr::from_str(f).unwrap());
        self
    }

    pub fn with_packages(mut self, packages: Option<&str>) -> Self {
        self.packages = packages.map(|f| serde_saphyr::from_str(f).unwrap());
        self
    }

    pub fn with_variables(mut self, variables: &str) -> Self {
        self.common = self.common.with_variables(variables);
        self
    }

    pub fn with_agent_type_id(mut self, agent_type_id: &str) -> Self {
        self.common.agent_type_id = AgentTypeID::try_from(agent_type_id).unwrap();
        self
    }

    pub fn without_deployment(mut self) -> Self {
        self.executables = None;
        self.health = None;
        self.filesystem = None;
        self.shared_filesystem = None;
        self
    }

    pub fn write(self, local_dir: PathBuf) -> String {
        self.common.write(local_dir.as_path(), &self.to_string())
    }
}
