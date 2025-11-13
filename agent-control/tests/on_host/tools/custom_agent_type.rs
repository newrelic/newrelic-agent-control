use std::fmt::Display;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use newrelic_agent_control::agent_type::agent_type_id::AgentTypeID;
pub const DYNAMIC_AGENT_TYPE_FILENAME: &str = "dynamic-agent-types/type.yaml";

/// Helper to build a Custom Agent type with defaults ready to use in integration tests
pub struct CustomAgentType {
    agent_type_id: AgentTypeID,
    variables: Option<serde_yaml::Value>,
    executables: Option<serde_yaml::Value>,
    health: Option<serde_yaml::Value>,
    version: Option<serde_yaml::Value>,
}

impl Default for CustomAgentType {
    fn default() -> Self {
        Self {
            agent_type_id: Self::default_agent_type_id(),
            variables: Some(
                serde_yaml::from_str(
                    r#"
fake_variable:
  description: "fake variable to verify remote config"
  type: "string"
  required: false
  default: "default"
"#,
                )
                .unwrap(),
            ),
            executables: Some(Self::default_executables()),
            version: Some(Self::default_version_checker()),
            health: None,
        }
    }
}

impl Display for CustomAgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let content = format!(
            r#"
        namespace: {}
        name: {}
        version: {}
        "#,
            self.agent_type_id.namespace(),
            self.agent_type_id.name(),
            self.agent_type_id.version()
        );
        let mut content: serde_yaml::Mapping = serde_yaml::from_str(&content).unwrap();
        let mut variables = serde_yaml::Mapping::new();
        if let Some(v) = self.variables.as_ref() {
            variables.insert("on_host".into(), v.clone());
        }
        let mut deployment_content = serde_yaml::Mapping::new();
        if let Some(executables) = self.executables.as_ref() {
            deployment_content.insert("executables".into(), executables.clone());
        }
        if let Some(health) = self.health.as_ref() {
            deployment_content.insert("health".into(), health.clone());
        }
        if let Some(version) = self.version.as_ref() {
            deployment_content.insert("version".into(), version.clone());
        }
        let mut deployment = serde_yaml::Mapping::new();
        deployment.insert("on_host".into(), deployment_content.into());
        content.insert("variables".into(), variables.into());
        content.insert("deployment".into(), deployment.into());
        let content = serde_yaml::Value::from(content);

        write!(f, "{}", serde_yaml::to_string(&content).unwrap())
    }
}

impl CustomAgentType {
    fn default_agent_type_id() -> AgentTypeID {
        AgentTypeID::try_from("newrelic/com.newrelic.custom_agent:0.1.0").unwrap()
    }

    #[cfg(target_family = "unix")]
    fn default_executables() -> serde_yaml::Value {
        serde_yaml::from_str(
            r#"
- id: "trap-term-sleep"
  path: "sh"
  args: "tests/on_host/data/trap_term_sleep_60.sh"
"#,
        )
        .unwrap()
    }

    #[cfg(target_family = "windows")]
    fn default_executables() -> serde_yaml::Value {
        serde_yaml::from_str(
            r#"
- id: "trap-term-sleep"
  path: "powershell.exe"
  args: "-NoProfile -ExecutionPolicy Bypass -File tests\\on_host\\data\\trap_term_sleep_60.ps1"
"#,
        )
        .unwrap()
    }

    #[cfg(target_family = "unix")]
    fn default_version_checker() -> serde_yaml::Value {
        serde_yaml::from_str(
            r#"
path: "echo"
args: "Some data 1.0.0 Some data"
regex: \d+\.\d+\.\d+
"#,
        )
        .unwrap()
    }

    #[cfg(target_family = "windows")]
    fn default_version_checker() -> serde_yaml::Value {
        serde_yaml::from_str(
            r#"
path: "cmd"
args: "/C echo Some data 1.0.0 Some data"
regex: \d+\.\d+\.\d+
"#,
        )
        .unwrap()
    }

    pub fn empty() -> Self {
        Self {
            agent_type_id: Self::default_agent_type_id(),
            variables: None,
            executables: None,
            health: None,
            version: None,
        }
    }

    pub fn with_executables(self, executables: Option<&str>) -> Self {
        Self {
            executables: executables.map(|e| serde_yaml::from_str(e).unwrap()),
            ..self
        }
    }

    pub fn with_health(self, health: Option<&str>) -> Self {
        Self {
            health: health.map(|h| serde_yaml::from_str(h).unwrap()),
            ..self
        }
    }

    pub fn with_version(self, version: Option<&str>) -> Self {
        Self {
            version: version.map(|v| serde_yaml::from_str(v).unwrap()),
            ..self
        }
    }

    pub fn without_deployment(self) -> Self {
        Self {
            executables: None,
            health: None,
            version: None,
            ..self
        }
    }

    /// Writes the custom agent type and returns its id as string.
    pub fn build(self, local_dir: PathBuf) -> String {
        let agent_type_file_path = local_dir.join(DYNAMIC_AGENT_TYPE_FILENAME);

        std::fs::create_dir_all(agent_type_file_path.parent().unwrap()).unwrap();
        let mut local_file =
            File::create(agent_type_file_path.clone()).expect("failed to create local config file");
        write!(local_file, "{self}").expect("failed to write custom agent type");
        self.agent_type_id.to_string()
    }
}
