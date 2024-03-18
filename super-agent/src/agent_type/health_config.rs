use serde::Deserialize;
use std::{collections::HashMap, time::Duration};

/// Represents the configuration for health checks.
///
/// This structure includes parameters to define intervals between health checks,
/// timeouts for checks, and the specific health check method—either HTTP or execute command.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct HealthConfig {
    /// The duration to wait between health checks.
    interval: Duration,

    /// The maximum duration a health check may run before considered failed.
    timeout: Duration,

    /// Details on the type of health check. Defined by the `HealthCheck` enumeration.
    #[serde(flatten)]
    check: HealthCheck,
}

/// Enumeration representing the possible types of health checks.
///
/// Variants include `HttpHealth` and `ExecHealth`, corresponding to health checks via HTTP and execute command, respectively.
enum HealthCheck {
    HttpHealth,
    ExecHealth,
}

/// Represents an HTTP-based health check.
///
/// For further details, refer to [Kubernetes documentation](https://kubernetes.io/docs/tasks/configure-pod-container/configure-liveness-readiness-startup-probes/).
pub struct HttpHealth {
    /// The HTTP path to check for the health check.
    path: String,

    /// The port to be checked during the health check.
    port: u8,

    /// Optional HTTP headers to be included during the health check.
    headers: Option<HashMap<String, String>>,
}

/// Represents a health check based on an executed command.
///
/// For further details, refer to [Kubernetes documentation](https://kubernetes.io/docs/tasks/configure-pod-container/configure-liveness-readiness-startup-probes/).
pub struct ExecHealth {
    /// The command to be executed for the health check.
    command: Vec<String>,
}
