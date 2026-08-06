//! Namespaces used to prefix agent type variable names (e.g. `nr-var`, `nr-env`, `nr-vault`).
use std::fmt::Display;
use strum::{EnumIter, IntoEnumIterator};

/// Holds the variable name prefixed with the namespace.
/// Example: "nr-env:MY_ENV_VAR" for the environment variable "MY_ENV_VAR".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VariableName {
    namespace: Namespace,
    name: String,
}

impl VariableName {
    /// Builds a [`VariableName`] from its namespace and unprefixed name.
    pub fn new(namespace: Namespace, name: impl Into<String>) -> Self {
        Self {
            namespace,
            name: name.into(),
        }
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }
}

impl Display for VariableName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(
            formatter,
            "{}{}{}",
            self.namespace,
            Namespace::PREFIX_NS_SEPARATOR,
            self.name
        )
    }
}

/// Namespace defines the supported namespace names for variables definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter)]
pub enum Namespace {
    /// Variables defined in the agent type.
    Variable,
    /// Attributes related to the sub-agent.
    SubAgent,
    /// Attributes related to the agent-control.
    AgentControl,

    /// Variables exposing particular paths when expanding user values.
    Path,

    // Below variables are "secret" variables.
    // These are loaded bye secret providers every time a remote config is received.
    /// Environment variables.
    EnvironmentVariable,
    /// Secrets retrieved from a HashiCorp Vault.
    Vault,
    /// Secrets retrieved from a file.
    File,
    /// Secrets retrieved from Kubernetes Secrets.
    K8sSecret,
}

impl Namespace {
    const PREFIX: &'static str = "nr-";
    /// Separator between a namespace prefix and the variable name.
    pub const PREFIX_NS_SEPARATOR: &'static str = ":";

    /// Encapsulates the variables defined in the agent-type
    const VARIABLE: &'static str = "var";
    /// Encapsulates attributes related to the sub-agent
    const SUB_AGENT: &'static str = "sub";
    /// Encapsulates attributes related to the agent-control
    const AC: &'static str = "ac";

    /// Encapsulates paths available to expanded user values
    const PATH: &'static str = "path";

    /// Encapsulates the environment variables
    const ENVIRONMENT_VARIABLE: &'static str = "env";
    /// Encapsulates the secrets retrieved from a HashiCorp Vault
    const VAULT_SECRET: &'static str = "vault";
    /// Encapsulates the secrets retrieved from K8s Secrets
    const K8S_SECRET: &'static str = "kubesec";
    const FILE_SECRET: &'static str = "file";

    /// Returns whether the given namespaced name belongs to a secret namespace.
    pub fn is_secret_variable(s: &str) -> bool {
        Self::iter()
            .filter(Namespace::is_secret)
            .any(|ns| s.starts_with(ns.to_string().as_str()))
    }

    /// Whether this namespace holds "secret" variables (loaded on every remote config fetch).
    fn is_secret(&self) -> bool {
        match self {
            Namespace::Variable
            | Namespace::SubAgent
            | Namespace::AgentControl
            | Namespace::Path => false,
            Namespace::EnvironmentVariable
            | Namespace::Vault
            | Namespace::File
            | Namespace::K8sSecret => true,
        }
    }
}

impl Display for Namespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ns = match self {
            Self::Variable => Self::VARIABLE,
            Self::SubAgent => Self::SUB_AGENT,
            Self::AgentControl => Self::AC,
            Self::Path => Self::PATH,
            Self::EnvironmentVariable => Self::ENVIRONMENT_VARIABLE,
            Self::Vault => Self::VAULT_SECRET,
            Self::File => Self::FILE_SECRET,
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
            VariableName::new(Namespace::Variable, "test").to_string()
        );
        assert_eq!(
            "nr-sub:test".to_string(),
            VariableName::new(Namespace::SubAgent, "test").to_string()
        );
        assert_eq!(
            "nr-env:test".to_string(),
            VariableName::new(Namespace::EnvironmentVariable, "test").to_string()
        );
        assert_eq!(
            "nr-ac:test".to_string(),
            VariableName::new(Namespace::AgentControl, "test").to_string()
        );
        assert_eq!(
            "nr-vault:test".to_string(),
            VariableName::new(Namespace::Vault, "test").to_string()
        );
        assert_eq!(
            "nr-kubesec:test".to_string(),
            VariableName::new(Namespace::K8sSecret, "test").to_string()
        );
        assert_eq!(
            "nr-file:test".to_string(),
            VariableName::new(Namespace::File, "test").to_string()
        );
    }
}
