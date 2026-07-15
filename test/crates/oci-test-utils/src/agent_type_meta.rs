//! Minimal agent-type metadata + OCI tag composition for the `oci-utils` CLI.
//!
//! This is a deliberate, hand-maintained mirror of the agent-control logic at
//! `agent-control/src/agent_type/oci.rs::AgentTypeTag` and the platform/OS →
//! environment mapping it relies on. We duplicate rather than import because
//! `oci-test-utils` must not take a dependency on `newrelic-agent-control` (see
//! NR-578592 design constraints).
//!
//! Keep this in sync with `AgentTypeTag::new` and its `environment_prefix` helper.
//! If a reviewer prefers to share validation (`AgentTypeID`'s name/version regexes
//! etc.), the change is local to this file.

use serde::Deserialize;
use thiserror::Error;

/// The minimal subset of an agent type definition needed to derive the OCI tag and
/// the archive's internal filename. Mirrors only the fields the CLI consumes — full
/// definition parsing lives in `newrelic-agent-control`.
#[derive(Debug, Deserialize)]
pub struct AgentTypeDefinitionMeta {
    /// Owning namespace (e.g. `"newrelic"`). Currently unused for tag composition but
    /// required by the YAML schema, so we parse and surface it for callers.
    pub namespace: String,
    /// Agent type name (e.g. `"com.newrelic.infrastructure"`).
    pub name: String,
    /// Version (e.g. `"0.1.0"`). Treated as an opaque string here; agent-control performs
    /// stricter validation in `AgentTypeID`.
    pub version: String,
    /// Deployment platform: `"host"` or `"kubernetes"`.
    pub platform: String,
    /// Host OS when `platform == "host"`. Omitted for kubernetes.
    #[serde(default)]
    pub operating_system: Option<String>,
}

/// Errors produced while reading or interpreting an agent-type definition YAML.
#[derive(Debug, Error)]
pub enum MetaError {
    /// The YAML failed to deserialize into the minimal metadata struct.
    #[error("failed to parse agent type definition: {0}")]
    Parse(String),
    /// The `(platform, operating_system)` pair didn't match a known environment.
    #[error("unsupported platform/os combination: platform={platform:?}, operating_system={os:?}")]
    UnsupportedEnvironment {
        /// Value of the `platform` field.
        platform: String,
        /// Value of the `operating_system` field (if any).
        os: Option<String>,
    },
}

impl AgentTypeDefinitionMeta {
    /// Parses the minimal metadata fields out of an agent type definition YAML.
    pub fn from_yaml_str(yaml: &str) -> Result<Self, MetaError> {
        serde_saphyr::from_str(yaml).map_err(|e| MetaError::Parse(e.to_string()))
    }

    /// Returns the OCI tag for this definition, matching the format Agent Control
    /// derives when pulling: `<environment-prefix>-<name>-<version>`.
    pub fn compose_tag(&self) -> Result<String, MetaError> {
        let prefix = environment_prefix(&self.platform, self.operating_system.as_deref())?;
        Ok(format!("{prefix}-{}-{}", self.name, self.version))
    }
}

/// Maps `(platform, operating_system)` to the fixed environment prefix used in the OCI tag.
///
/// Mirrors `agent-control/src/agent_type/oci.rs::environment_prefix`:
/// ```text
///   ("host",       Some("linux"))   → "host-linux"
///   ("host",       Some("windows")) → "host-windows"
///   ("kubernetes", None)            → "kubernetes"
/// ```
fn environment_prefix(platform: &str, os: Option<&str>) -> Result<&'static str, MetaError> {
    match (platform, os) {
        ("host", Some("linux")) => Ok("host-linux"),
        ("host", Some("windows")) => Ok("host-windows"),
        ("kubernetes", None) => Ok("kubernetes"),
        _ => Err(MetaError::UnsupportedEnvironment {
            platform: platform.to_string(),
            os: os.map(str::to_string),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(yaml: &str) -> AgentTypeDefinitionMeta {
        AgentTypeDefinitionMeta::from_yaml_str(yaml).expect("yaml should parse")
    }

    #[test]
    fn tag_host_linux() {
        let m = meta(
            "
namespace: newrelic
name: com.newrelic.infrastructure
version: 0.1.0
platform: host
operating_system: linux
",
        );
        assert_eq!(
            m.compose_tag().unwrap(),
            "host-linux-com.newrelic.infrastructure-0.1.0"
        );
    }

    #[test]
    fn tag_host_windows() {
        let m = meta(
            "
namespace: newrelic
name: com.newrelic.infrastructure
version: 0.1.0
platform: host
operating_system: windows
",
        );
        assert_eq!(
            m.compose_tag().unwrap(),
            "host-windows-com.newrelic.infrastructure-0.1.0"
        );
    }

    #[test]
    fn tag_kubernetes_no_os() {
        let m = meta(
            "
namespace: newrelic
name: com.newrelic.infrastructure
version: 0.1.0
platform: kubernetes
",
        );
        assert_eq!(
            m.compose_tag().unwrap(),
            "kubernetes-com.newrelic.infrastructure-0.1.0"
        );
    }

    #[test]
    fn unsupported_platform_rejected() {
        let m = meta(
            "
namespace: newrelic
name: com.newrelic.infrastructure
version: 0.1.0
platform: serverless
",
        );
        assert!(matches!(
            m.compose_tag(),
            Err(MetaError::UnsupportedEnvironment { .. })
        ));
    }

    #[test]
    fn host_without_os_rejected() {
        let m = meta(
            "
namespace: newrelic
name: com.newrelic.infrastructure
version: 0.1.0
platform: host
",
        );
        assert!(matches!(
            m.compose_tag(),
            Err(MetaError::UnsupportedEnvironment { .. })
        ));
    }

    #[test]
    fn kubernetes_with_os_rejected() {
        let m = meta(
            "
namespace: newrelic
name: com.newrelic.infrastructure
version: 0.1.0
platform: kubernetes
operating_system: linux
",
        );
        assert!(matches!(
            m.compose_tag(),
            Err(MetaError::UnsupportedEnvironment { .. })
        ));
    }
}
