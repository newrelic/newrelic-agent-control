//! Health-check configuration for on-host agents (HTTP or file-based checks).
use super::templateable_value::TemplateableValue;
use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::templates::Templateable;
use crate::checkers::health::health_checker::{HealthCheckInterval, InitialDelay};
use duration_str::deserialize_duration;
use serde::{Deserialize, Deserializer};
use std::{collections::HashMap, time::Duration};
use wrapper_with_default::WrapperWithDefault;

const DEFAULT_HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(15);

pub mod rendered;

/// Represents the configuration for health checks.
///
/// Defines the periodicity/timeout parameters and an explicit list of checks that the on-host
/// supervisor should run. An empty (or omitted) `checks` list disables health reporting.
#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct OnHostHealthConfig {
    /// The duration to wait between health checks.
    #[serde(default)]
    pub(crate) interval: HealthCheckInterval,

    /// The initial delay before the first health check is performed.
    #[serde(default)]
    pub(crate) initial_delay: InitialDelay,

    /// The maximum duration a health check may run before considered failed.
    #[serde(default)]
    pub(crate) timeout: HealthCheckTimeout,

    /// The list of health checks to run. Empty (or absent) means health reporting is disabled.
    #[serde(default, deserialize_with = "deserialize_checks")]
    pub(crate) checks: Vec<OnHostHealthCheckDefinition>,
}

/// Deserializes `checks` and rejects agent-type definitions that declare the `Process` kind more
/// than once: the runtime `try_new` can only wire a single supervised-executable consumer.
fn deserialize_checks<'de, D>(deserializer: D) -> Result<Vec<OnHostHealthCheckDefinition>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    let checks = <Vec<OnHostHealthCheckDefinition>>::deserialize(deserializer)?;
    let process_count = checks
        .iter()
        .filter(|c| matches!(c, OnHostHealthCheckDefinition::Process))
        .count();
    if process_count > 1 {
        return Err(D::Error::custom(format!(
            "the 'Process' on-host health check may be declared at most once, found {process_count}"
        )));
    }
    Ok(checks)
}

/// The maximum duration a health check may run before being considered failed.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_HEALTH_CHECK_TIMEOUT)]
pub struct HealthCheckTimeout(#[serde(deserialize_with = "deserialize_duration")] Duration);

/// A single on-host health check declaration.
#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(tag = "kind")]
pub(crate) enum OnHostHealthCheckDefinition {
    Process,
    Http(HttpHealth),
    File(FileHealth),
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub(crate) struct FileHealth {
    pub(crate) path: String,
}

impl Templateable for FileHealth {
    type Output = Self;
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let rendered = self.path.template_with(variables)?;
        Ok(Self { path: rendered })
    }
}

/// Represents an HTTP-based port.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub(crate) struct HttpPort(pub(super) u16);

impl From<HttpPort> for u16 {
    fn from(value: HttpPort) -> Self {
        value.0
    }
}

impl From<u16> for HttpPort {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

impl Default for HttpPort {
    fn default() -> Self {
        Self(80)
    }
}

/// Represents an HTTP-based health check.
///
/// For further details, refer to [Kubernetes documentation](https://kubernetes.io/docs/tasks/configure-pod-container/configure-liveness-readiness-startup-probes/).
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub(crate) struct HttpHealth {
    #[serde(default)]
    pub(crate) host: TemplateableValue<HttpHost>,

    /// The HTTP path to check for the health check.
    pub(crate) path: TemplateableValue<HttpPath>,

    /// The port to be checked during the health check.
    pub(crate) port: TemplateableValue<HttpPort>,

    /// Optional HTTP headers to be included during the health check.
    #[serde(default)]
    pub(crate) headers: HashMap<String, String>,

    // allowed healthy HTTP status codes
    #[serde(default)]
    pub(crate) healthy_status_codes: Vec<u16>,
}

impl Default for HttpHealth {
    fn default() -> Self {
        Self {
            host: TemplateableValue::new(HttpHost::default()),
            path: TemplateableValue::new(HttpPath::default()),
            port: TemplateableValue::new(HttpPort::default()),
            headers: HashMap::default(),
            healthy_status_codes: vec![],
        }
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub(crate) struct HttpHost(String);

impl Default for HttpHost {
    fn default() -> Self {
        Self("127.0.0.1".to_string())
    }
}

impl From<HttpHost> for String {
    fn from(value: HttpHost) -> Self {
        value.0
    }
}

impl From<String> for HttpHost {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub(crate) struct HttpPath(String);

impl Default for HttpPath {
    fn default() -> Self {
        Self("/".to_string())
    }
}

impl From<HttpPath> for String {
    fn from(value: HttpPath) -> Self {
        value.0
    }
}

impl From<String> for HttpPath {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl Templateable for HttpPath {
    type Output = Self;

    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_string = self.0.template_with(variables)?;
        Ok(Self(templated_string))
    }
}

impl Templateable for OnHostHealthConfig {
    type Output = rendered::OnHostHealthConfig;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        Ok(Self::Output {
            checks: self
                .checks
                .into_iter()
                .map(|c| c.template_with(variables))
                .collect::<Result<Vec<_>, _>>()?,
            interval: self.interval,
            initial_delay: self.initial_delay,
            timeout: self.timeout,
        })
    }
}

impl Templateable for OnHostHealthCheckDefinition {
    type Output = rendered::OnHostHealthCheckDefinition;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        Ok(match self {
            OnHostHealthCheckDefinition::Process => rendered::OnHostHealthCheckDefinition::Process,
            OnHostHealthCheckDefinition::Http(conf) => {
                let health_conf = rendered::HttpHealth {
                    host: conf.host.template_with(variables)?,
                    path: conf.path.template_with(variables)?,
                    port: conf.port.template_with(variables)?,
                    headers: conf.headers,
                    healthy_status_codes: conf.healthy_status_codes,
                };
                rendered::OnHostHealthCheckDefinition::Http(health_conf)
            }
            OnHostHealthCheckDefinition::File(conf) => {
                let health_conf = FileHealth {
                    path: conf.path.template_with(variables)?,
                };
                rendered::OnHostHealthCheckDefinition::File(health_conf)
            }
        })
    }
}

impl Templateable for TemplateableValue<HttpPort> {
    type Output = HttpPort;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        let templated_string = self.template.template_with(variables)?;
        let value = if templated_string.is_empty() {
            HttpPort::default()
        } else {
            templated_string
                .parse::<u16>()
                .map(HttpPort)
                .map_err(|_| AgentTypeError::ValueNotParseableFromString(templated_string))?
        };
        Ok(value)
    }
}

impl Templateable for TemplateableValue<HttpHost> {
    type Output = HttpHost;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        let templated_string = self.template.template_with(variables)?;
        let value = if templated_string.is_empty() {
            HttpHost::default()
        } else {
            HttpHost(templated_string)
        };
        Ok(value)
    }
}

impl Templateable for TemplateableValue<HttpPath> {
    type Output = HttpPath;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        let templated_string = self.template.template_with(variables)?;
        let value = if templated_string.is_empty() {
            HttpPath::default()
        } else {
            HttpPath(templated_string)
        };
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_the_three_kinds() {
        let yaml = r#"
interval: 5s
initial_delay: 1s
timeout: 3s
checks:
  - kind: Process
  - kind: Http
    path: /healthz
    port: 8080
  - kind: File
    path: /var/lib/health.yaml
"#;
        let cfg: OnHostHealthConfig = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(cfg.checks.len(), 3);
        assert!(matches!(
            cfg.checks[0],
            OnHostHealthCheckDefinition::Process
        ));
        assert!(matches!(
            cfg.checks[1],
            OnHostHealthCheckDefinition::Http(_)
        ));
        assert!(matches!(
            cfg.checks[2],
            OnHostHealthCheckDefinition::File(_)
        ));
    }

    #[test]
    fn empty_or_missing_checks_defaults_to_empty_list() {
        let cfg: OnHostHealthConfig = serde_saphyr::from_str("interval: 5s\n").unwrap();
        assert!(cfg.checks.is_empty());

        let cfg: OnHostHealthConfig = serde_saphyr::from_str("interval: 5s\nchecks: []\n").unwrap();
        assert!(cfg.checks.is_empty());
    }

    #[test]
    fn rejects_unknown_kind() {
        let yaml = r#"
checks:
  - kind: Bogus
"#;
        assert!(serde_saphyr::from_str::<OnHostHealthConfig>(yaml).is_err());
    }

    #[test]
    fn rejects_duplicate_process_check_at_parse_time() {
        let yaml = r#"
checks:
  - kind: Process
  - kind: Process
"#;
        let err = serde_saphyr::from_str::<OnHostHealthConfig>(yaml)
            .expect_err("duplicate Process kind must be rejected at parse time");
        let msg = err.to_string();
        assert!(
            msg.contains("'Process'") && msg.contains("at most once"),
            "unexpected error message: {msg}"
        );
    }
}
