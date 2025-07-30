use std::fmt::Display;

/// Holds the variable name prefixed with the namespace.
/// Example: "nr-env:MY_ENV_VAR" for the environment variable "MY_ENV_VAR".
pub type NamespacedVariableName = String;

/// Namespace defines the supported namespace names for variables definition.
#[derive(PartialEq, Eq, Hash)]
pub enum Namespace {
    Variable,
    SubAgent,
    AgentControl,

    // Below variables are "secret" variables.
    // These are loaded every time a remote config is received.
    EnvironmentVariable,
    Vault,
    K8sSecret,
}

impl Namespace {
    const PREFIX: &'static str = "nr-";
    pub const PREFIX_NS_SEPARATOR: &'static str = ":";

    /// Encapsulates the variables defined in the agent-type
    const VARIABLE: &'static str = "var";
    /// Encapsulates attributes related to the sub-agent
    const SUB_AGENT: &'static str = "sub";
    /// Encapsulates attributes related to the agent-control
    const AC: &'static str = "ac";

    /// Encapsulates the environment variables
    const ENVIRONMENT_VARIABLE: &'static str = "env";
    /// Encapsulates the secrets retrieved from a HashiCorp Vault
    const VAULT_SECRET: &'static str = "vault";
    /// Encapsulates the secrets retrieved from K8s Secrets
    const K8S_SECRET: &'static str = "kubesec";

    pub fn namespaced_name(&self, name: &str) -> NamespacedVariableName {
        format!("{}{}{}", self, Self::PREFIX_NS_SEPARATOR, name)
    }

    pub fn is_secret_variable(s: &str) -> bool {
        [
            Namespace::Vault,
            Namespace::K8sSecret,
            Namespace::EnvironmentVariable,
        ]
        .iter()
        .any(|ns| s.starts_with(ns.to_string().as_str()))
    }
}

impl Display for Namespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ns = match self {
            Self::Variable => Self::VARIABLE,
            Self::SubAgent => Self::SUB_AGENT,
            Self::AgentControl => Self::AC,
            Self::EnvironmentVariable => Self::ENVIRONMENT_VARIABLE,
            Self::Vault => Self::VAULT_SECRET,
            Self::K8sSecret => Self::K8S_SECRET,
        };
        write!(f, "{}{ns}", Self::PREFIX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_namespaced_name() {
        assert_eq!(
            "nr-var:test".to_string(),
            Namespace::Variable.namespaced_name("test")
        );
        assert_eq!(
            "nr-sub:test".to_string(),
            Namespace::SubAgent.namespaced_name("test")
        );
        assert_eq!(
            "nr-env:test".to_string(),
            Namespace::EnvironmentVariable.namespaced_name("test")
        );
        assert_eq!(
            "nr-ac:test".to_string(),
            Namespace::AgentControl.namespaced_name("test")
        );
        assert_eq!(
            "nr-vault:test".to_string(),
            Namespace::Vault.namespaced_name("test")
        );
        assert_eq!(
            "nr-kubesec:test".to_string(),
            Namespace::K8sSecret.namespaced_name("test")
        );
    }
}
