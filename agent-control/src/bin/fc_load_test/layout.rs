//! On-disk layout of every simulated Agent Control and the files written into it.
use crate::config::FakeAgentTypeId;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_ID, DYNAMIC_AGENT_TYPES_DIR, FOLDER_NAME_LOCAL_DATA,
};
use newrelic_agent_control::opamp::instance_id::on_host::identifiers::Identifiers;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Registry address that refuses connections, so unexpected OCI downloads fail fast.
const UNREACHABLE_OCI_REGISTRY: &str = "127.0.0.1:1";
const HEALTHY_FILE_NAME: &str = "healthy.yaml";
const LOCAL_CONFIG_FILE_NAME: &str = "local_config.yaml";

/// Directories of one simulated Agent Control.
#[derive(Debug, Clone)]
pub struct AcLayout {
    /// Simulated host name, also used as host id and directory name.
    pub name: String,
    pub local_dir: PathBuf,
    pub remote_dir: PathBuf,
    pub log_dir: PathBuf,
}

impl AcLayout {
    pub fn new(data_root: &Path, run_prefix: &str, index: usize) -> Self {
        let name = format!("{run_prefix}-{index:04}");
        let base = data_root.join(&name);
        Self {
            local_dir: base.join("local"),
            remote_dir: base.join("remote"),
            log_dir: base.join("log"),
            name,
        }
    }

    /// Path of the local Agent Control config file.
    pub fn local_config_path(&self) -> PathBuf {
        self.local_dir
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(AGENT_CONTROL_ID)
            .join(LOCAL_CONFIG_FILE_NAME)
    }
}

/// A fake agent type ready to be written into a dynamic agent types directory.
#[derive(Debug, Clone)]
pub struct FakeAgentType {
    pub file_name: String,
    pub yaml: String,
}

/// Identifiers reported by the simulated host `name`. Always the same for the same input, so
/// instance ids are reused across runs.
pub fn identifiers(name: &str, fleet_id: &str) -> Identifiers {
    Identifiers {
        hostname: name.to_string(),
        machine_id: name.to_string(),
        cloud_instance_id: String::new(),
        host_id: name.to_string(),
        fleet_id: fleet_id.to_string(),
    }
}

/// Removes the data of the simulated Agent Controls in `indices` and nothing else, so other
/// prefixes and other processes splitting the same run keep their identities.
pub fn wipe_range(data_root: &Path, run_prefix: &str, indices: Range<usize>) -> io::Result<()> {
    for index in indices {
        let dir = data_root.join(AcLayout::new(data_root, run_prefix, index).name);
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
    }
    Ok(())
}

/// Writes the file read by every fake file health check and returns its path.
pub fn write_healthy_file(data_root: &Path) -> io::Result<PathBuf> {
    std::fs::create_dir_all(data_root)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = data_root.join(HEALTHY_FILE_NAME);
    let content = format!(
        "healthy: true\nstatus: \"load-test fake healthy\"\nstart_time_unix_nano: {now}\nstatus_time_unix_nano: {now}\n"
    );
    std::fs::write(&path, content)?;
    Ok(path)
}

/// Builds a no-op on-host agent type reporting healthy through a file health check.
pub fn fake_agent_type(
    id: &FakeAgentTypeId,
    healthy_file: &Path,
    interval: Duration,
) -> FakeAgentType {
    let FakeAgentTypeId {
        namespace,
        name,
        version,
    } = id;
    // JSON strings are valid YAML scalars, which keeps arbitrary paths safe.
    let path = serde_json::to_string(&healthy_file.to_string_lossy()).unwrap_or_default();
    let interval = interval.as_secs().max(1);
    let yaml = format!(
        r#"namespace: {namespace}
name: {name}
version: {version}
platform: host
operating_system: linux
protocol_version: "1.0"
variables: {{}}
deployment:
  health:
    interval: {interval}s
    checks:
      - kind: File
        path: {path}
"#
    );
    FakeAgentType {
        file_name: format!("{namespace}-{name}-{version}.yaml"),
        yaml,
    }
}

/// Creates the directories of a simulated Agent Control and writes its local config (the
/// template with forced per-host fields) and the fake agent types.
pub fn prepare(
    layout: &AcLayout,
    template: &serde_json::Value,
    fake_agent_types: &[FakeAgentType],
) -> io::Result<()> {
    std::fs::create_dir_all(&layout.remote_dir)?;
    std::fs::create_dir_all(&layout.log_dir)?;

    let mut config = template.clone();
    let object = config
        .as_object_mut()
        .ok_or_else(|| io::Error::other("agent_control template must be a map"))?;
    object.insert("host_id".to_string(), layout.name.clone().into());
    object.insert(
        "self_update".to_string(),
        serde_json::json!({ "enabled": false }),
    );
    // One status server per simulated host would clash on the same port.
    object.insert(
        "server".to_string(),
        serde_json::json!({ "enabled": false }),
    );
    let oci = object.entry("oci").or_insert_with(|| serde_json::json!({}));
    let oci = oci
        .as_object_mut()
        .ok_or_else(|| io::Error::other("agent_control.oci must be a map"))?;
    oci.insert("registry".to_string(), UNREACHABLE_OCI_REGISTRY.into());

    let config_path = layout.local_config_path();
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let yaml = serde_saphyr::to_string(&config).map_err(io::Error::other)?;
    std::fs::write(config_path, yaml)?;

    let agent_types_dir = layout.local_dir.join(DYNAMIC_AGENT_TYPES_DIR);
    std::fs::create_dir_all(&agent_types_dir)?;
    for fake in fake_agent_types {
        std::fs::write(agent_types_dir.join(&fake.file_name), &fake.yaml)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use newrelic_agent_control::agent_control::agent_id::AgentID;
    use newrelic_agent_control::agent_control::config::AgentControlConfig;
    use newrelic_agent_control::agent_type::agent_type_id::AgentTypeID;
    use newrelic_agent_control::agent_type::definition::AgentTypeDefinition;
    use newrelic_agent_control::agent_type::registry::{AgentTypeRegistry, AgentTypeRegistryError};
    use newrelic_agent_control::agent_type::variable::value::VariableValues;
    use newrelic_agent_control::sub_agent::agent_renderer::{AgentRenderer, Renderer};
    use newrelic_agent_control::sub_agent::identity::AgentIdentity;
    use newrelic_agent_control::value_provider::ValueProviders;
    use newrelic_agent_control::values::yaml_config::YAMLConfig;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn infra_id() -> FakeAgentTypeId {
        FakeAgentTypeId {
            namespace: "newrelic".to_string(),
            name: "com.newrelic.infrastructure".to_string(),
            version: "0.1.0".to_string(),
        }
    }

    fn template() -> serde_json::Value {
        serde_json::json!({
            "host_id": "real-host",
            "self_update": { "enabled": true },
            "oci": { "registry": "real.example" },
            "server": { "enabled": true },
            "fleet_control": {
                "endpoint": "http://localhost/v1/opamp",
                "poll_interval": "30s",
                "fleet_id": "f1",
            },
            "agents": {},
        })
    }

    #[test]
    fn layout_paths() {
        let layout = AcLayout::new(Path::new("/data"), "lt", 7);
        assert_eq!(layout.name, "lt-0007");
        assert_eq!(layout.local_dir, PathBuf::from("/data/lt-0007/local"));
        assert_eq!(layout.remote_dir, PathBuf::from("/data/lt-0007/remote"));
        assert_eq!(layout.log_dir, PathBuf::from("/data/lt-0007/log"));
        assert_eq!(
            layout.local_config_path(),
            PathBuf::from("/data/lt-0007/local/local-data/agent-control/local_config.yaml")
        );
    }

    #[test]
    fn identifiers_are_deterministic() {
        let ids = identifiers("lt-0007", "f1");
        assert_eq!(ids, identifiers("lt-0007", "f1"));
        assert_eq!(ids.hostname, "lt-0007");
        assert_eq!(ids.host_id, "lt-0007");
        assert_eq!(ids.machine_id, "lt-0007");
        assert_eq!(ids.cloud_instance_id, "");
        assert_eq!(ids.fleet_id, "f1");
    }

    #[test]
    fn forced_fields_override_template() {
        let root = tempdir().unwrap();
        let layout = AcLayout::new(root.path(), "lt", 1);

        prepare(&layout, &template(), &[]).unwrap();

        let content = std::fs::read_to_string(layout.local_config_path()).unwrap();
        let config: AgentControlConfig = serde_saphyr::from_str(&content).unwrap();
        assert_eq!(config.host_id, "lt-0001");
        assert!(!bool::from(config.self_update.enabled));
        assert_eq!(config.oci.registry.to_string(), UNREACHABLE_OCI_REGISTRY);
        assert!(!config.server.enabled);
        assert_eq!(config.fleet_control.unwrap().fleet_id, "f1");
        assert!(layout.remote_dir.is_dir());
        assert!(layout.log_dir.is_dir());
    }

    #[test]
    fn prepare_writes_fake_agent_types() {
        let root = tempdir().unwrap();
        let layout = AcLayout::new(root.path(), "lt", 1);
        let fake = fake_agent_type(
            &infra_id(),
            Path::new("/tmp/h.yaml"),
            Duration::from_secs(5),
        );

        prepare(&layout, &template(), std::slice::from_ref(&fake)).unwrap();

        let written = layout
            .local_dir
            .join(DYNAMIC_AGENT_TYPES_DIR)
            .join(&fake.file_name);
        assert_eq!(std::fs::read_to_string(written).unwrap(), fake.yaml);
    }

    #[test]
    fn fake_type_parses_with_file_health() {
        let fake = fake_agent_type(
            &infra_id(),
            Path::new("/tmp/h.yaml"),
            Duration::from_secs(5),
        );
        let definition = AgentTypeDefinition::from_slice(fake.yaml.as_bytes()).unwrap();

        assert_eq!(
            definition.agent_type_id(),
            &AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.1.0").unwrap()
        );
        let runtime = serde_json::to_value(&definition.runtime_config).unwrap();
        let host = &runtime["deployment"]["Host"];
        assert_eq!(host["executables"], serde_json::json!([]), "{runtime}");
        assert_eq!(host["packages"], serde_json::json!({}), "{runtime}");
        assert_eq!(
            host["health"]["checks"],
            serde_json::json!([{ "kind": "File", "path": "/tmp/h.yaml" }]),
            "{runtime}"
        );
    }

    struct SingleTypeRegistry(AgentTypeDefinition);
    impl AgentTypeRegistry for SingleTypeRegistry {
        fn get(&self, _: &AgentTypeID) -> Result<AgentTypeDefinition, AgentTypeRegistryError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn shadowed_type_ignores_real_values() {
        let remote = tempdir().unwrap();
        let fake = fake_agent_type(
            &infra_id(),
            Path::new("/tmp/h.yaml"),
            Duration::from_secs(5),
        );
        let definition = AgentTypeDefinition::from_slice(fake.yaml.as_bytes()).unwrap();
        let identity = AgentIdentity::from((
            AgentID::try_from("infra").unwrap(),
            definition.agent_type_id().clone(),
        ));
        let renderer = AgentRenderer::new(
            Arc::new(SingleTypeRegistry(definition)),
            VariableValues::default(),
            ValueProviders::default(),
            remote.path(),
        );
        // Values Fleet Control would push for the real infrastructure agent type.
        let values = YAMLConfig::try_from(
            "config_agent:\n  license_key: abc\nversion: 1.2.3\nhealth_port: 18003\n".to_string(),
        )
        .unwrap();

        renderer.render_agent(&identity, values).unwrap();
    }

    #[test]
    fn healthy_file_is_healthy() {
        let root = tempdir().unwrap();
        let path = write_healthy_file(root.path()).unwrap();
        let content: serde_json::Value =
            serde_saphyr::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(content["healthy"], serde_json::json!(true));
        assert!(content["status_time_unix_nano"].as_u64().unwrap() > 0);
    }

    #[test]
    fn fresh_wipes_only_own_index_range() {
        let root = tempdir().unwrap();
        for dir in ["lt-0001", "lt-0002", "lt-0003", "lt-other-0001", "ltx-0001"] {
            std::fs::create_dir(root.path().join(dir)).unwrap();
        }
        std::fs::write(root.path().join(HEALTHY_FILE_NAME), "").unwrap();

        wipe_range(root.path(), "lt", 1..3).unwrap();

        let mut left: Vec<String> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        left.sort();
        assert_eq!(
            left,
            vec!["healthy.yaml", "lt-0003", "lt-other-0001", "ltx-0001"]
        );
    }

    #[test]
    fn wipe_range_without_data_root_is_ok() {
        let root = tempdir().unwrap();
        wipe_range(&root.path().join("missing"), "lt", 0..3).unwrap();
    }
}
