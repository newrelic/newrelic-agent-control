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
    pub(super) interval: Duration,

    /// The maximum duration a health check may run before considered failed.
    #[serde(deserialize_with = "deserialize_duration")]
    pub(super) timeout: Duration,

    /// Details on the type of health check. Defined by the `HealthCheck` enumeration.
    #[serde(flatten)]
    pub(super) check: HealthCheck,
}

/// Enumeration representing the possible types of health checks.
///
/// Variants include `HttpHealth` and `ExecHealth`, corresponding to health checks via HTTP and execute command, respectively.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub(super) enum HealthCheck {
    #[serde(rename = "httpGet")]
    HttpGetHealth(HttpHealth),
    #[serde(rename = "exec")]
    ExecHealth(ExecHealth),
}

/// Represents an HTTP-based port.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub(super) struct HttpPort(pub(super) u16);

/// Represents an HTTP-based health check.
///
/// For further details, refer to [Kubernetes documentation](https://kubernetes.io/docs/tasks/configure-pod-container/configure-liveness-readiness-startup-probes/).
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub(super) struct HttpHealth {
    /// The HTTP path to check for the health check.
    pub(super) path: String,

    /// The port to be checked during the health check.
    pub(super) port: TemplateableValue<HttpPort>,

    /// Optional HTTP headers to be included during the health check.
    pub(super) headers: Option<HashMap<String, String>>,
}

/// Represents a health check based on an executed command.
///
/// For further details, refer to [Kubernetes documentation](https://kubernetes.io/docs/tasks/configure-pod-container/configure-liveness-readiness-startup-probes/).
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub(super) struct ExecHealth {
    /// The binary path to be executed for the health check.
    path: String,
    /// Arguments provided to the executed command.
    args: Vec<String>,
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
            HealthCheck::HttpGetHealth(ref conf) => {
                let mut conf = conf.clone();
                conf.port = conf.port.template_with(variables)?;
                HealthCheck::HttpGetHealth(conf)
            }
            _ => self,
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
