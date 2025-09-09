use duration_str::deserialize_duration;
use serde::Deserialize;
use std::{collections::HashMap, time::Duration};
use wrapper_with_default::WrapperWithDefault;

use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::templates::Templateable;
use crate::health::health_checker::{HealthCheckInterval, InitialDelay};

use super::templateable_value::TemplateableValue;

const DEFAULT_HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(15);

/// Represents the configuration for health checks.
///
/// This structure includes parameters to define intervals between health checks,
/// timeouts for checks, and the specific health check method—either HTTP or execute command.
#[derive(Debug, Deserialize, Clone, PartialEq)]
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

    /// Details on the type of health check. Defined by the `HealthCheck` enumeration.
    #[serde(flatten)]
    pub(crate) check: OnHostHealthCheck,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_HEALTH_CHECK_TIMEOUT)]
pub struct HealthCheckTimeout(#[serde(deserialize_with = "deserialize_duration")] Duration);

/// Enumeration representing the possible types of health checks.
///
/// Variants include `HttpHealth` and `ExecHealth`, corresponding to health checks via HTTP and execute command, respectively.
#[derive(Debug, Deserialize, Clone, PartialEq)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum OnHostHealthCheck {
    ExecHealth,
    #[serde(rename = "http")]
    HttpHealth(HttpHealth),
    #[serde(rename = "file")]
    FileHealth(FileHealth),
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub(crate) struct FileHealth {
    pub(crate) path: String,
}

impl Templateable for FileHealth {
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
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_string = self.0.template_with(variables)?;
        Ok(Self(templated_string))
    }
}

impl Templateable for OnHostHealthConfig {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            check: self.check.template_with(variables)?,
            ..self
        })
    }
}

impl Templateable for OnHostHealthCheck {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(match self {
            OnHostHealthCheck::ExecHealth => OnHostHealthCheck::ExecHealth,
            OnHostHealthCheck::HttpHealth(conf) => {
                let health_conf = HttpHealth {
                    host: conf.host.template_with(variables)?,
                    path: conf.path.template_with(variables)?,
                    port: conf.port.template_with(variables)?,
                    ..conf
                };
                OnHostHealthCheck::HttpHealth(health_conf)
            }
            OnHostHealthCheck::FileHealth(conf) => {
                let health_conf = FileHealth {
                    path: conf.path.template_with(variables)?,
                };
                OnHostHealthCheck::FileHealth(health_conf)
            }
        })
    }
}

impl Templateable for TemplateableValue<HttpPort> {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(variables)?;
        let value = if templated_string.is_empty() {
            HttpPort::default()
        } else {
            templated_string
                .parse::<u16>()
                .map(HttpPort)
                .map_err(|_| AgentTypeError::ValueNotParseableFromString(templated_string))?
        };
        Ok(Self {
            template: self.template,
            value: Some(value),
        })
    }
}

impl Templateable for TemplateableValue<HttpHost> {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(variables)?;
        let value = if templated_string.is_empty() {
            HttpHost::default()
        } else {
            HttpHost(templated_string)
        };
        Ok(Self {
            template: self.template,
            value: Some(value),
        })
    }
}

impl Templateable for TemplateableValue<HttpPath> {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(variables)?;
        let value = if templated_string.is_empty() {
            HttpPath::default()
        } else {
            HttpPath(templated_string)
        };
        Ok(Self {
            template: self.template,
            value: Some(value),
        })
    }
}
