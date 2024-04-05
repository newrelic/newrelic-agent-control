use duration_str::deserialize_duration;
use serde::Deserialize;
use std::{collections::HashMap, time::Duration};

use super::{
    definition::{TemplateableValue, Variables},
    error::AgentTypeError,
    runtime_config_templates::Templateable,
};

/// Represents the configuration for health checks.
///
/// This structure includes parameters to define intervals between health checks,
/// timeouts for checks, and the specific health check method—either HTTP or execute command.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct HealthConfig {
    /// The duration to wait between health checks.
    #[serde(deserialize_with = "deserialize_duration")]
    pub(crate) interval: Duration,

    /// The maximum duration a health check may run before considered failed.
    #[serde(deserialize_with = "deserialize_duration")]
    pub(crate) timeout: Duration,

    /// Details on the type of health check. Defined by the `HealthCheck` enumeration.
    #[serde(flatten)]
    pub(crate) check: HealthCheck,
}

/// Enumeration representing the possible types of health checks.
///
/// Variants include `HttpHealth` and `ExecHealth`, corresponding to health checks via HTTP and execute command, respectively.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub(crate) enum HealthCheck {
    #[serde(rename = "http")]
    HttpHealth(HttpHealth),
    // #[serde(rename = "exec")]
    // ExecHealth(ExecHealth),
}

/// Represents an HTTP-based port.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub(crate) struct HttpPort(pub(super) u16);

impl From<HttpPort> for u16 {
    fn from(value: HttpPort) -> Self {
        value.0
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

impl Templateable for HttpHost {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_string = self.0.template_with(variables)?;
        Ok(Self(templated_string))
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

impl Templateable for HttpPath {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_string = self.0.template_with(variables)?;
        Ok(Self(templated_string))
    }
}

/// Represents a health check based on an executed command.
///
/// For further details, refer to [Kubernetes documentation](https://kubernetes.io/docs/tasks/configure-pod-container/configure-liveness-readiness-startup-probes/).
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub(crate) struct ExecHealth {
    /// The binary path to be executed for the health check.
    pub(crate) path: String,
    /// Arguments provided to the executed command.
    pub(crate) args: Vec<String>,
    // allowed healthy exit codes
    pub(crate) healthy_exit_codes: Vec<i32>,
}

impl Templateable for HealthConfig {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            check: self.check.template_with(variables)?,
            ..self
        })
    }
}

impl Templateable for HealthCheck {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(match self {
            HealthCheck::HttpHealth(ref conf) => {
                let mut conf = conf.clone();
                conf.port = conf.port.template_with(variables)?;
                HealthCheck::HttpHealth(conf)
            }
        })
    }
}

impl Templateable for TemplateableValue<HttpPort> {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        let templated_string = self.template.clone().template_with(variables)?;
        let value = if templated_string.is_empty() {
            return Err(AgentTypeError::MissingDefault);
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
